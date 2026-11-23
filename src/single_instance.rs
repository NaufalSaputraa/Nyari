use crate::NyariError;
use std::mem::size_of;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS,
    ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER, ERROR_IO_PENDING, ERROR_NOT_FOUND,
    ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, ERROR_SUCCESS, FALSE, GENERIC_ALL,
    GENERIC_WRITE, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_SID,
    TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows::Win32::Security::{
    GetTokenInformation, InitializeSecurityDescriptor, IsValidSid, SetSecurityDescriptorDacl,
    TokenUser, ACL, NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentProcess, OpenProcessToken, ResetEvent, SetEvent,
    WaitForMultipleObjects, WaitForSingleObject, INFINITE,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const PRIMARY_MUTEX_NAME: &str = r"Local\NyariBrowser_Instance";
const WAIT_PIPE_STEP_MS: u32 = 1;
const WAIT_PIPE_MAX_MS: u64 = 500;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Owned Win32 `HANDLE` wrapper that guarantees `CloseHandle` on drop.
struct HandleGuard(HANDLE);

impl HandleGuard {
    fn new(handle: HANDLE) -> Result<Self, NyariError> {
        if handle.is_invalid() || handle == INVALID_HANDLE_VALUE {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(Self(handle))
    }

    fn as_raw(&self) -> HANDLE {
        self.0
    }

    fn into_raw(mut self) -> HANDLE {
        let handle = self.0;
        self.0 = INVALID_HANDLE_VALUE;
        handle
    }
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Owned `LocalAlloc` wide-string buffer freed via `LocalFree` on drop.
struct LocalPwstrGuard(PWSTR);

impl Drop for LocalPwstrGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0.cast())));
            }
        }
    }
}

/// Transfers an owned `HandleGuard` across thread boundaries safely.
struct SendHandleGuard(HandleGuard);

unsafe impl Send for SendHandleGuard {}

/// Transfers a borrowed Win32 event handle across thread boundaries safely.
struct SendHandle(HANDLE);

unsafe impl Send for SendHandle {}

/// Owns a `SECURITY_ATTRIBUTES` struct and its backing heap-allocated `SECURITY_DESCRIPTOR`
/// and `LocalAlloc`-allocated DACL (`LocalFree`d on drop).
pub struct SidDaclGuard {
    sd_ptr: *mut SECURITY_DESCRIPTOR,
    dacl: *mut ACL,
    security_attributes: SECURITY_ATTRIBUTES,
}

unsafe impl Send for SidDaclGuard {}
unsafe impl Sync for SidDaclGuard {}

impl SidDaclGuard {
    /// Returns a pointer to the owned `SECURITY_ATTRIBUTES` valid for the lifetime of `self`.
    pub fn as_security_attributes(&self) -> *const SECURITY_ATTRIBUTES {
        &self.security_attributes
    }
}

impl Drop for SidDaclGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.dacl.is_null() {
                let _ = LocalFree(Some(HLOCAL(self.dacl.cast())));
            }
            if !self.sd_ptr.is_null() {
                drop(Box::from_raw(self.sd_ptr));
            }
        }
    }
}

/// Guards primary-instance ownership and stops the named-pipe listener cleanly on drop.
pub struct SingleInstanceGuard {
    mutex_handle: HANDLE,
    stop_event: HANDLE,
    listener_thread: Option<JoinHandle<()>>,
}

unsafe impl Send for SingleInstanceGuard {}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.stop_event);
        }
        if let Some(handle) = self.listener_thread.take() {
            let _ = handle.join();
        }
        unsafe {
            let _ = CloseHandle(self.stop_event);
            let _ = CloseHandle(self.mutex_handle);
        }
    }
}

/// Formats the per-user single-instance named pipe path for the given SID string.
pub fn pipe_name_for_sid(sid: &str) -> String {
    format!(r"\\.\pipe\NyariBrowser_Instance_{sid}")
}

/// Queries the current process token `TokenUser` and returns the user's string SID (e.g. `S-1-5-...`).
pub fn user_sid_string() -> Result<String, NyariError> {
    let token_buf = query_current_user_token_buf()?;
    let sid = token_user_sid(&token_buf)?;

    let mut raw_pwstr = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut raw_pwstr)?;
    }
    let _pwstr_guard = LocalPwstrGuard(raw_pwstr);

    let wide = unsafe { raw_pwstr.as_wide() };
    String::from_utf16(wide)
        .map_err(|err| NyariError::Other(format!("invalid UTF-16 SID string: {err}")))
}

/// Creates a `SECURITY_ATTRIBUTES` whose DACL grants `GENERIC_ALL` exclusively to the current user SID.
pub fn create_user_sid_dacl_security_attributes() -> Result<SidDaclGuard, NyariError> {
    let token_buf = query_current_user_token_buf()?;
    let sid = token_user_sid(&token_buf)?;

    let ea = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL.0,
        grfAccessMode: SET_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: Default::default(),
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: PWSTR(sid.0.cast()),
        },
    };

    let mut dacl: *mut ACL = std::ptr::null_mut();
    let acl_err = unsafe { SetEntriesInAclW(Some(&[ea]), None, &mut dacl) };
    if acl_err != ERROR_SUCCESS {
        return Err(win32_error_to_nyari(acl_err));
    }

    // Heap-allocate SECURITY_DESCRIPTOR so moving SidDaclGuard never invalidates lpSecurityDescriptor.
    let sd_ptr = Box::into_raw(Box::new(SECURITY_DESCRIPTOR::default()));
    let mut guard = SidDaclGuard {
        sd_ptr,
        dacl,
        security_attributes: SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd_ptr.cast(),
            bInheritHandle: FALSE,
        },
    };

    unsafe {
        let sd = PSECURITY_DESCRIPTOR(guard.sd_ptr.cast());
        InitializeSecurityDescriptor(sd, SECURITY_DESCRIPTOR_REVISION)?;
        SetSecurityDescriptorDacl(sd, true, Some(guard.dacl), false)?;
    }
    guard.security_attributes.lpSecurityDescriptor = guard.sd_ptr.cast();

    Ok(guard)
}

/// Acquires the default single-instance mutex (`Local\NyariBrowser_Instance`) or forwards `urls`
/// to the running primary instance via the user-SID DACL-locked named pipe.
pub fn acquire_or_forward(
    urls: &[String],
    on_message: impl Fn(Vec<String>) + Send + 'static,
) -> Result<Option<SingleInstanceGuard>, NyariError> {
    let sid = user_sid_string()?;
    let pipe_name = pipe_name_for_sid(&sid);
    acquire_or_forward_named(PRIMARY_MUTEX_NAME, &pipe_name, urls, on_message)
}

/// Acquires `mutex_name` or forwards `urls` to `pipe_name` if another primary instance holds it.
pub fn acquire_or_forward_named(
    mutex_name: &str,
    pipe_name: &str,
    urls: &[String],
    on_message: impl Fn(Vec<String>) + Send + 'static,
) -> Result<Option<SingleInstanceGuard>, NyariError> {
    if mutex_name.is_empty() || pipe_name.is_empty() {
        return Err(NyariError::InvalidInput(
            "mutex_name and pipe_name must be non-empty".into(),
        ));
    }

    let dacl_guard = create_user_sid_dacl_security_attributes()?;
    let mutex_wide = to_wide_null(mutex_name);

    let mutex_raw = unsafe {
        CreateMutexW(
            Some(dacl_guard.as_security_attributes()),
            false,
            PCWSTR(mutex_wide.as_ptr()),
        )?
    };
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex_guard = HandleGuard::new(mutex_raw)?;

    if already_exists {
        drop(mutex_guard);
        forward_urls_to_primary(pipe_name, urls)?;
        return Ok(None);
    }

    // Primary instance creates the DACL-locked pipe server immediately before returning.
    let pipe_wide = to_wide_null(pipe_name);
    let pipe_raw = unsafe {
        CreateNamedPipeW(
            PCWSTR(pipe_wide.as_ptr()),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            Some(dacl_guard.as_security_attributes()),
        )
    };
    let pipe_guard = HandleGuard::new(pipe_raw)?;

    let stop_event_raw = unsafe { CreateEventW(None, true, false, None)? };
    let stop_event_guard = HandleGuard::new(stop_event_raw)?;

    let send_pipe = SendHandleGuard(pipe_guard);
    let send_stop = SendHandle(stop_event_guard.as_raw());

    let listener_thread = thread::Builder::new()
        .name("nyari-instance-pipe".to_string())
        .spawn(move || {
            run_pipe_listener(send_pipe, send_stop, on_message);
        })?;

    Ok(Some(SingleInstanceGuard {
        mutex_handle: mutex_guard.into_raw(),
        stop_event: stop_event_guard.into_raw(),
        listener_thread: Some(listener_thread),
    }))
}

fn query_current_user_token_buf() -> Result<Vec<u64>, NyariError> {
    let mut raw_token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token)?;
    }
    let token = HandleGuard::new(raw_token)?;

    let mut needed = 0u32;
    let first_err = unsafe { GetTokenInformation(token.as_raw(), TokenUser, None, 0, &mut needed) };
    if let Err(err) = first_err {
        if err.code() != HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) {
            return Err(err.into());
        }
    }
    if needed < size_of::<TOKEN_USER>() as u32 {
        return Err(NyariError::Other("TokenUser buffer size too small".into()));
    }

    // Back buffer with u64 words so pointer meets x86_64 alignment for TOKEN_USER.
    let word_len = (needed as usize).div_ceil(size_of::<u64>());
    let mut buf = vec![0u64; word_len];
    let buf_bytes = u32::try_from(word_len * size_of::<u64>())
        .map_err(|_| NyariError::Other("TokenUser buffer size overflow".into()))?;
    unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            buf_bytes,
            &mut needed,
        )?;
    }
    Ok(buf)
}

fn token_user_sid(token_buf: &[u64]) -> Result<PSID, NyariError> {
    const _: () = assert!(std::mem::align_of::<TOKEN_USER>() <= std::mem::align_of::<u64>());
    if std::mem::size_of_val(token_buf) < size_of::<TOKEN_USER>() {
        return Err(NyariError::Other("truncated TOKEN_USER buffer".into()));
    }
    debug_assert!(token_buf.as_ptr().cast::<TOKEN_USER>().is_aligned());
    let token_user = unsafe { &*(token_buf.as_ptr().cast::<TOKEN_USER>()) };
    let sid = token_user.User.Sid;
    if sid.0.is_null() || !unsafe { IsValidSid(sid) }.as_bool() {
        return Err(NyariError::Other(
            "invalid user SID in process token".into(),
        ));
    }
    Ok(sid)
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_error_to_nyari(err: WIN32_ERROR) -> NyariError {
    windows::core::Error::from_hresult(HRESULT::from_win32(err.0)).into()
}

fn forward_urls_to_primary(pipe_name: &str, urls: &[String]) -> Result<(), NyariError> {
    let mut payload = serde_json::to_vec(urls)?;
    payload.push(b'\n');

    let pipe_wide = to_wide_null(pipe_name);
    let deadline = Instant::now() + Duration::from_millis(WAIT_PIPE_MAX_MS);

    loop {
        let ready = unsafe { WaitNamedPipeW(PCWSTR(pipe_wide.as_ptr()), WAIT_PIPE_STEP_MS) };
        if ready.as_bool() {
            match unsafe {
                CreateFileW(
                    PCWSTR(pipe_wide.as_ptr()),
                    GENERIC_WRITE.0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            } {
                Ok(handle) => {
                    let pipe_guard = HandleGuard::new(handle)?;
                    write_all_to_pipe(pipe_guard.as_raw(), &payload)?;
                    unsafe {
                        FlushFileBuffers(pipe_guard.as_raw())?;
                    }
                    return Ok(());
                }
                Err(err) => {
                    if err.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0)
                        || Instant::now() >= deadline
                    {
                        return Err(err.into());
                    }
                }
            }
        } else {
            let err = unsafe { GetLastError() };
            if err == ERROR_ACCESS_DENIED || Instant::now() >= deadline {
                return Err(win32_error_to_nyari(err));
            }
            thread::sleep(Duration::from_millis(u64::from(WAIT_PIPE_STEP_MS)));
        }
    }
}

fn write_all_to_pipe(pipe: HANDLE, mut data: &[u8]) -> Result<(), NyariError> {
    while !data.is_empty() {
        let mut written = 0u32;
        unsafe {
            WriteFile(pipe, Some(data), Some(&mut written), None)?;
        }
        if written == 0 {
            return Err(NyariError::Other("wrote 0 bytes to named pipe".into()));
        }
        data = &data[written as usize..];
    }
    Ok(())
}

fn run_pipe_listener(
    pipe_send: SendHandleGuard,
    stop_send: SendHandle,
    on_message: impl Fn(Vec<String>) + Send + 'static,
) {
    let pipe_guard = pipe_send.0;
    let stop_event = stop_send.0;

    let Ok(io_event_raw) = (unsafe { CreateEventW(None, true, false, None) }) else {
        return;
    };
    let Ok(io_event_guard) = HandleGuard::new(io_event_raw) else {
        return;
    };

    loop {
        if is_event_signaled(stop_event) {
            break;
        }

        match connect_overlapped(pipe_guard.as_raw(), stop_event, io_event_guard.as_raw()) {
            Ok(true) => {
                read_client_messages(
                    pipe_guard.as_raw(),
                    stop_event,
                    io_event_guard.as_raw(),
                    &on_message,
                );
                unsafe {
                    let _ = DisconnectNamedPipe(pipe_guard.as_raw());
                }
            }
            Ok(false) => break,
            Err(_) => {
                unsafe {
                    let _ = DisconnectNamedPipe(pipe_guard.as_raw());
                }
                if is_event_signaled(stop_event) {
                    break;
                }
            }
        }
    }
}

fn is_event_signaled(event: HANDLE) -> bool {
    unsafe { WaitForSingleObject(event, 0) == WAIT_OBJECT_0 }
}

fn cancel_and_drain_overlapped(pipe: HANDLE, overlapped: &OVERLAPPED) {
    let cancel_res = unsafe { CancelIoEx(pipe, Some(overlapped)) };
    let should_drain = match cancel_res {
        Ok(()) => true,
        Err(err) => err.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0),
    };
    if should_drain {
        let mut transferred = 0u32;
        if let Err(err) = unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, true) } {
            // ERROR_OPERATION_ABORTED is expected once cancellation completes.
            let _ = err.code() == HRESULT::from_win32(ERROR_OPERATION_ABORTED.0);
        }
    }
}

fn connect_overlapped(
    pipe: HANDLE,
    stop_event: HANDLE,
    io_event: HANDLE,
) -> Result<bool, NyariError> {
    unsafe {
        ResetEvent(io_event)?;
    }
    let mut overlapped = OVERLAPPED {
        hEvent: io_event,
        ..Default::default()
    };

    match unsafe { ConnectNamedPipe(pipe, Some(&mut overlapped)) } {
        Ok(()) => Ok(true),
        Err(err) => {
            if err.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) {
                Ok(true)
            } else if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) {
                let handles = [stop_event, io_event];
                let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if wait == WAIT_OBJECT_0 {
                    cancel_and_drain_overlapped(pipe, &overlapped);
                    Ok(false)
                } else if wait.0 == WAIT_OBJECT_0.0 + 1 {
                    let mut transferred = 0u32;
                    unsafe {
                        GetOverlappedResult(pipe, &overlapped, &mut transferred, false)?;
                    }
                    Ok(true)
                } else {
                    cancel_and_drain_overlapped(pipe, &overlapped);
                    Err(windows::core::Error::from_thread().into())
                }
            } else {
                Err(err.into())
            }
        }
    }
}

fn read_client_messages(
    pipe: HANDLE,
    stop_event: HANDLE,
    io_event: HANDLE,
    on_message: &impl Fn(Vec<String>),
) {
    let mut line_buf = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        if is_event_signaled(stop_event) {
            return;
        }
        if unsafe { ResetEvent(io_event) }.is_err() {
            return;
        }
        let mut overlapped = OVERLAPPED {
            hEvent: io_event,
            ..Default::default()
        };
        let mut bytes_read = 0u32;

        let read_res = unsafe {
            ReadFile(
                pipe,
                Some(&mut chunk),
                Some(&mut bytes_read),
                Some(&mut overlapped),
            )
        };

        let n = match read_res {
            Ok(()) => {
                let mut transferred = 0u32;
                if unsafe { GetOverlappedResult(pipe, &overlapped, &mut transferred, false) }
                    .is_ok()
                {
                    transferred as usize
                } else {
                    bytes_read as usize
                }
            }
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                let handles = [stop_event, io_event];
                let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if wait == WAIT_OBJECT_0 {
                    cancel_and_drain_overlapped(pipe, &overlapped);
                    return;
                }
                if wait.0 != WAIT_OBJECT_0.0 + 1 {
                    cancel_and_drain_overlapped(pipe, &overlapped);
                    break;
                }
                let mut transferred = 0u32;
                if unsafe { GetOverlappedResult(pipe, &overlapped, &mut transferred, false) }
                    .is_err()
                {
                    break;
                }
                transferred as usize
            }
            Err(err) if err.code() == HRESULT::from_win32(ERROR_BROKEN_PIPE.0) => break,
            Err(_) => break,
        };

        if n == 0 {
            break;
        }
        if line_buf.len().saturating_add(n) > MAX_MESSAGE_BYTES {
            line_buf.clear();
            break;
        }
        line_buf.extend_from_slice(&chunk[..n]);

        while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
            let raw_line: Vec<u8> = line_buf.drain(..=pos).collect();
            dispatch_json_line(&raw_line, on_message);
        }
    }

    if !line_buf.is_empty() {
        dispatch_json_line(&line_buf, on_message);
    }
}

fn dispatch_json_line(raw: &[u8], on_message: &impl Fn(Vec<String>)) {
    let trimmed = std::str::from_utf8(raw).ok().map(str::trim);
    if let Some(line) = trimmed.filter(|s| !s.is_empty()) {
        if let Ok(urls) = serde_json::from_str::<Vec<String>>(line) {
            on_message(urls);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_or_forward_named, create_user_sid_dacl_security_attributes, pipe_name_for_sid,
        user_sid_string,
    };
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::Security::{
        GetSecurityDescriptorDacl, IsValidSecurityDescriptor, ACL, PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES,
    };

    #[test]
    fn pipe_name_for_sid_formats_expected_path() {
        assert_eq!(
            pipe_name_for_sid("S-1-5-21-1000-2000-3000-1001"),
            r"\\.\pipe\NyariBrowser_Instance_S-1-5-21-1000-2000-3000-1001"
        );
    }

    #[test]
    fn user_sid_string_returns_valid_non_empty_sid() {
        let sid = user_sid_string().expect("user_sid_string should succeed");
        assert!(!sid.is_empty());
        assert!(sid.starts_with("S-1-"), "unexpected SID format: {sid}");
    }

    #[test]
    fn create_user_sid_dacl_security_attributes_returns_valid_dacl() {
        let guard = create_user_sid_dacl_security_attributes()
            .expect("create_user_sid_dacl_security_attributes should succeed");
        let sa = guard.as_security_attributes();
        assert!(!sa.is_null());
        unsafe {
            let sa_ref = &*sa;
            assert_eq!(
                sa_ref.nLength,
                std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32
            );
            assert!(!sa_ref.lpSecurityDescriptor.is_null());
            let sd = PSECURITY_DESCRIPTOR(sa_ref.lpSecurityDescriptor);
            assert!(IsValidSecurityDescriptor(sd).as_bool());

            let mut dacl_present = windows::core::BOOL(0);
            let mut dacl_ptr: *mut ACL = std::ptr::null_mut();
            let mut dacl_defaulted = windows::core::BOOL(0);
            GetSecurityDescriptorDacl(sd, &mut dacl_present, &mut dacl_ptr, &mut dacl_defaulted)
                .expect("GetSecurityDescriptorDacl should succeed");
            assert!(dacl_present.as_bool());
            assert!(!dacl_ptr.is_null());
        }
    }

    #[test]
    fn acquire_or_forward_named_primary_and_secondary_handover() {
        let id = uuid::Uuid::new_v4();
        let mutex_name = format!(r"Local\NyariBrowser_Test_Mutex_{id}");
        let sid = user_sid_string().expect("sid");
        let pipe_name = format!(r"\\.\pipe\NyariBrowser_Test_Pipe_{sid}_{id}");

        let (tx, rx) = mpsc::channel::<Vec<String>>();

        let primary = acquire_or_forward_named(&mutex_name, &pipe_name, &[], move |urls| {
            let _ = tx.send(urls);
        })
        .expect("primary acquire should succeed");
        let guard = primary.expect("primary should receive SingleInstanceGuard");

        // Secondary instance from another thread must detect existing mutex and forward URLs.
        let mutex_clone = mutex_name.clone();
        let pipe_clone = pipe_name.clone();
        let secondary_handle = std::thread::spawn(move || {
            acquire_or_forward_named(
                &mutex_clone,
                &pipe_clone,
                &["https://github.com".to_string()],
                |_| {},
            )
        });

        let secondary_res = secondary_handle
            .join()
            .expect("secondary thread panicked")
            .expect("secondary forward should succeed");
        assert!(secondary_res.is_none(), "secondary should return Ok(None)");

        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("primary should receive forwarded URLs");
        assert_eq!(received, vec!["https://github.com".to_string()]);

        // A second handover on the same primary pipe should also succeed.
        let secondary_two = acquire_or_forward_named(
            &mutex_name,
            &pipe_name,
            &["https://example.com".to_string()],
            |_| {},
        )
        .expect("second forward should succeed");
        assert!(secondary_two.is_none());

        let received_two = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("primary should receive second forwarded URLs");
        assert_eq!(received_two, vec!["https://example.com".to_string()]);

        // Dropping primary guard must stop the pipe listener and release the mutex cleanly.
        drop(guard);

        let (tx2, _rx2) = mpsc::channel::<Vec<String>>();
        let reacquired = acquire_or_forward_named(&mutex_name, &pipe_name, &[], move |urls| {
            let _ = tx2.send(urls);
        })
        .expect("reacquire after drop should succeed");
        assert!(
            reacquired.is_some(),
            "new primary should acquire after drop"
        );
    }

    #[test]
    fn acquire_or_forward_named_rejects_empty_names() {
        let res = acquire_or_forward_named("", r"\\.\pipe\test", &[], |_| {});
        assert!(matches!(res, Err(crate::NyariError::InvalidInput(_))));
        let res2 = acquire_or_forward_named(r"Local\test", "", &[], |_| {});
        assert!(matches!(res2, Err(crate::NyariError::InvalidInput(_))));
    }

    #[test]
    fn acquire_or_forward_named_concurrent_cold_start_race() {
        use std::sync::{Arc, Barrier};

        let id = uuid::Uuid::new_v4();
        let mutex_name = format!(r"Local\NyariBrowser_Race_Mutex_{id}");
        let sid = user_sid_string().expect("sid");
        let pipe_name = format!(r"\\.\pipe\NyariBrowser_Race_Pipe_{sid}_{id}");

        let barrier = Arc::new(Barrier::new(2));
        let (tx, rx) = mpsc::channel::<Vec<String>>();

        let spawn_racer = |url: &str| {
            let b = Arc::clone(&barrier);
            let m = mutex_name.clone();
            let p = pipe_name.clone();
            let t = tx.clone();
            let u = vec![url.to_string()];
            std::thread::spawn(move || {
                b.wait();
                acquire_or_forward_named(&m, &p, &u, move |urls| {
                    let _ = t.send(urls);
                })
            })
        };

        let h1 = spawn_racer("https://one.example");
        let h2 = spawn_racer("https://two.example");

        let r1 = h1.join().expect("thread 1 panicked").expect("r1 ok");
        let r2 = h2.join().expect("thread 2 panicked").expect("r2 ok");

        match (r1, r2) {
            (Some(_guard), None) => {
                let forwarded = rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("primary should receive thread 2 URLs");
                assert_eq!(forwarded, vec!["https://two.example".to_string()]);
            }
            (None, Some(_guard)) => {
                let forwarded = rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("primary should receive thread 1 URLs");
                assert_eq!(forwarded, vec!["https://one.example".to_string()]);
            }
            (Some(_), Some(_)) => panic!("both racers acquired primary mutex!"),
            (None, None) => panic!("neither racer acquired primary mutex!"),
        }
    }

    #[test]
    fn drop_cancels_pending_read_cleanly() {
        use super::{to_wide_null, write_all_to_pipe, HandleGuard};
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::GENERIC_WRITE;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
        };
        use windows::Win32::System::Pipes::WaitNamedPipeW;

        let id = uuid::Uuid::new_v4();
        let mutex_name = format!(r"Local\NyariBrowser_CancelRead_Mutex_{id}");
        let sid = user_sid_string().expect("sid");
        let pipe_name = format!(r"\\.\pipe\NyariBrowser_CancelRead_Pipe_{sid}_{id}");

        let (tx, rx) = mpsc::channel::<Vec<String>>();
        let guard = acquire_or_forward_named(&mutex_name, &pipe_name, &[], move |urls| {
            let _ = tx.send(urls);
        })
        .expect("primary acquire should succeed")
        .expect("should own primary guard");

        // Keep client handle open after writing one line so the server parks in overlapped ReadFile.
        let pipe_wide = to_wide_null(&pipe_name);
        let ready = unsafe { WaitNamedPipeW(PCWSTR(pipe_wide.as_ptr()), 500) };
        assert!(ready.as_bool(), "pipe should become ready");
        let client_raw = unsafe {
            CreateFileW(
                PCWSTR(pipe_wide.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .expect("client pipe open should succeed");
        let _client = HandleGuard::new(client_raw).expect("valid client handle");

        write_all_to_pipe(_client.as_raw(), b"[\"https://pending-read.example\"]\n")
            .expect("write should succeed");
        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("primary should receive line while client stays open");
        assert_eq!(received, vec!["https://pending-read.example".to_string()]);

        // Dropping guard while ReadFile is pending must cancel & drain IRP cleanly without hang/UB.
        drop(guard);
    }
}
