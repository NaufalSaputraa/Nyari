# Nyari Browser — AI Agent & Developer Guardrails (Hard Technical Invariants)

> **PENTING UNTUK AGENT / KONTRIBUTOR:** Dokumen ini memuat **50 aturan teknis mutlak** (*hard constraints* — v3.3 Builder-Check Verified) yang tidak boleh dilanggar dalam penulisan kode Rust maupun Preact. Setiap perubahan kode wajib mematuhi aturan di bawah ini untuk mencegah bug kompilasi `webview2-com 0.39.1`, cacat komposisi Win32/DPI/WM_ACTIVATE, blokir anti-bot CDP, kegagalan ekstensi, maupun celah keamanan dan kriptografi.

---

## 1. Aturan Backend Rust, Win32, DPI V2 & COM STA

1. **Strict COM STA Isolation:**
   * Seluruh instansiasi dan pemanggilan objek `ICoreWebView2*` dan Win32 `HWND` **WAJIB** berada di Main Thread (COM STA: `COINIT_APARTMENTTHREADED`).
   * **DILARANG KERAS** memanggil `futures::executor::block_on` atau memblokir thread UI dari Tokio background task. Gunakan `tao::event_loop::EventLoopProxy` untuk mengirim pesan satu arah dari `tokio` ke UI thread.

2. **Single-Instance Mutex, Immediate DACL Named Pipe & `--mcp-stdio` Flag:**
   * Jika dijalankan dengan argumen `--mcp-stdio`, proses **DILARANG** mengambil mutex UI atau membuat window; segera jalankan *headless stdio-to-Named-Pipe forwarder* ke `\\.\pipe\NyariBrowser_MCP_<SID>`.
   * Pada mode browser normal, periksa Win32 Named Mutex (`Local\NyariBrowser_Instance`). Instans utama **WAJIB** langsung membuat server Named Pipe (`\\.\pipe\NyariBrowser_Instance_<SID>`, dikunci DACL SID pengguna) **sebelum** membuat window atau WebView2.
   * Instans kedua wajib memanggil `WaitNamedPipeW` (retry 1 ms, maks 500 ms), mengirim argumen via Named Pipe, dan segera `std::process::exit(0)` dalam **< 10 ms**.

3. **Lazy Per-Workspace Named Profiles & Dedicated Private Profile (100% API Stabil):**
   * **DILARANG** memakai `CustomDataPartitionId` atau deklarasi vtable COM `ICoreWebView2Experimental*` (tidak tersedia di `webview2-com 0.39.1` stabil).
   * Jendela *Shell UI* (Layer 1 & Layer 2 Shell Overlay) berjalan di profil `default` (`ProfileName = nullptr`) agar jendela tampil **< 500 ms**.
   * Seluruh tab browsing reguler berjalan pada **Named Profile per Workspace (`nyari-ws-{id}`)** melalui `ICoreWebView2ControllerOptions::put_ProfileName`, diinisialisasi asinkron/lazy setelah Shell UI tampil. Penghapusan Workspace wajib memanggil `ICoreWebView2Profile8::Delete()`.
   * Seluruh **Private Tab (`Ctrl+Shift+N`)** wajib memakai **`ProfileName = "nyari-private"`** dan **`put_IsInPrivateModeEnabled(TRUE)`** (terlepas dari penghapusan Workspace, bebas ekstensi, dan **dilarang** dicatat ke `session.json` atau `history.db`).

4. **Dedicated SQLite Actor Thread & Bounded WAL Checkpoint:**
   * **DILARANG** membuka koneksi `rusqlite::Connection` bersamaan dari UI thread dan Tokio thread. Seluruh operasi `history.db` dan `vault.db` wajib dikelola oleh **satu SQLite Actor Thread khusus** (`std::sync::mpsc::sync_channel`).
   * Pada `WM_CLOSE`, UI thread mengirim `DbCommand::ShutdownAndCheckpoint` dengan timeout maksimal **150 ms** agar `PRAGMA wal_checkpoint(TRUNCATE)` bebas dari `SQLITE_BUSY` dan deadlock.

5. **3-Tier Memory Lifecycle, `nyari-tab://` `BrowsingInstance` Separation, `TrySuspend` & Dirty-Form Guard:**
   * Sebelum menidurkan tab ke Tier 1, wajib mengambil snapshot `{ scrollX, scrollY, hasUnsavedInput }` via one-shot CDP (lalu segera `Runtime.disable`) selagi renderer masih aktif.
   * Sebelum memanggil `TrySuspend`, validasi `IsVisible == false` dan pastikan tab tidak sedang memutar audio aktif. Wajib periksa nilai `isSuccessful`: jika `false`, tandai `soft_sleep_degraded` dan gunakan `MemoryUsageTargetLevel::LOW` + best-effort `K32EmptyWorkingSet`.
   * Panggil `K32EmptyWorkingSet` hanya jika 100% tab pada `PID` tersebut (`GetProcessExtendedInfos`) sedang tidur, dan tangani error `0x5` (`ERROR_ACCESS_DENIED`) secara senyap untuk proses AppContainer.
   * Klik tengah (`auxclick`) / `Ctrl+Klik` pada tautan wajib dicegat skrip pra-kompilasi ke `nyari-tab://open?url=...` (dibatalkan di `NavigationStarting`) untuk membuka tab controller independen yang memutus `BrowsingInstance`.
   * **Tier 2 Hard Unload** (`controller.Close()`) **WAJIB** mengecualikan tab Pinned, `keepAwake`, dan tab dengan `hasUnsavedInput == true`.

6. **Sequenced Deferral pada `NewWindowRequested` & Source-Profile Inheritance:**
   * Pre-compile skrip injeksi `Shield`, YouTube pruner, `nyari-glance://`, dan `nyari-tab://` di memori saat startup.
   * Handler `NewWindowRequested` **WAJIB** mengikuti urutan persis (setup sinkron STA < 5 ms, penyelesaian callback async < 150 ms):
     1. Ambil `let deferral = args.GetDeferral()?`
     2. Periksa `args.WindowFeatures()` (`HasSize` / `HasPosition` / suppressed chrome) untuk menentukan **OAuth / Sized Popup Window (`isOAuthPopup: true`, persistent owned top-level window)** vs tab reguler di `Content Host HWND` (`isOAuthPopup: false`).
     3. Buat `ICoreWebView2Controller` baru pada **profil yang sama persis dengan controller sumber** (`nyari-ws-{id}` untuk tab Workspace, atau `"nyari-private"` jika dipicu dari Private Tab/Private Glance; `DefaultBackgroundColor.A = 255`).
     4. Pasang guard navigasi `nyari://`, `nyari-glance://`, & `nyari-tab://`, `IsWebMessageEnabled(FALSE)`, `AreHostObjectsAllowed(FALSE)`, dan listener `Shield`.
     5. Daftarkan skrip injeksi pre-compiled via `AddScriptToExecuteOnDocumentCreated`.
     6. Tetapkan `args.put_NewWindow(new_webview)?`.
     7. Panggil `deferral.Complete()?`.
   * Registrasikan listener `WindowCloseRequested` untuk menutup controller OAuth saat `window.close()` dipanggil.

7. **Frameless Windowing, DPI V2, `Layer 1` L-Shaped Geometry, Owned `WS_POPUP` Priority Stack, Window-Family `WM_ACTIVATE`, & Dynamic `SetWindowRgn`:**
   * Deklarasikan `PerMonitorV2` (`DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`). Seluruh konstanta layout adalah **DIP (Logical Pixels @ 96 DPI)**; wajib dikalikan `GetDpiForWindow(hwnd) as f64 / 96.0` pada `SetWindowPos`, `BeginDeferWindowPos`, `put_Bounds()`, dan `SetWindowRgn`. Tangani `WM_DPICHANGED` + `put_RasterizationScale`.
   * Induk jendela Win32 wajib dibuat dengan `WS_EX_NOREDIRECTIONBITMAP`.
   * **DILARANG** menumpuk *sibling* `WS_CHILD` `HWND` transparan di atas `Content Host HWND`: Layer 1 dan Content Views wajib non-tumpang-tindih. Di mode vertikal (`vertical-expanded` / `vertical-collapsed`), `Layer 1` membentuk geometri berbentuk-L (`32 DIP` top caption strip + left sidebar via `SetWindowRgn`) sehingga Window Controls dan `46x32 DIP` `Snap Overlay HWND` selalu berada di pojok kanan atas `Layer 1` (dan disembunyikan bersama `Layer 1` via `SW_HIDE` saat `zen_mode == true`).
   * Seluruh overlay mengambang wajib di-host pada **Owned Top-Level Windows** (`hwndParent = main_hwnd`): `Shell Overlay Popup` (`Layer 2`), `Glance Popup`, `Extension Action Popup`, dan `OAuth Popup Window`.
   * `Shell Overlay Popup` (`Layer 2`) hanya menampilkan **1 surface aktif** berdasarkan Priority Stack (`P1 Modal FIFO -> P2 Command Palette / Find-in-Page -> P3 Reading Mode -> P4 Hover-Sidebar Drawer`) melalui `SET_OVERLAY_BOUNDS`. `Find-in-Page` (`Ctrl+F`) dihosting secara seragam di `Layer 2` pada semua mode layout.
   * Pada `WM_ACTIVATE(WA_INACTIVE)`, wajib periksa `GetAncestor(lparam_hwnd, GA_ROOT)`: klik di dalam owned popup tidak pernah menutup popup; klik kembali ke `main_hwnd`/`Content Host` hanya menutup overlay transien (`CommandPalette`, `Hover-Sidebar`, `Extension Action Popup`, `Glance`) sementara P1 Modal Dialog, Reading Mode, dan **OAuth Popup Window** tetap terbuka hingga diselesaikan; perpindahan keluar Nyari Window Family (`Alt+Tab`) menyembunyikan popup transien.
   * Tangani `WM_NCCALCSIZE` (clamp `MonitorInfo.rcWork`), Win10 `DwmExtendFrameIntoClientArea(cyTopHeight: 1)`, `WM_ERASEBKGND` (`1`), dan `WM_MOVE`/`WM_MOVING` (`NotifyParentWindowPositionChanged()`).
   * Pada `WM_WINDOWPOSCHANGED`: sembunyikan Resize Border Ring (`ShowWindow(SW_HIDE)`) jika `Maximized` atau `HTML5 Fullscreen`; saat `Restored`, hitung ulang `CombineRgn(outer, inner, RGN_DIFF)` dan panggil `SetWindowRgn(..., FALSE)`.

8. **Shortcut Routing & Focus Recovery:**
   * Event `AcceleratorKeyPressed` wajib disaring ketat: hanya proses jika `KeyEventKind == KeyDown | SystemKeyDown`. Kirim `SHORTCUT_TRIGGERED` ke Shell UI karena `Content Host HWND` memegang fokus keyboard saat browsing.
   * Setel `put_IsBrowserAcceleratorKeyEnabled(FALSE)` agar pintasan web diteruskan dengan benar.
   * Saat Owned Overlay Popup ditutup, Rust **WAJIB** memanggil `controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)` ke tab yang aktif.

---

## 2. Aturan Privasi, Keamanan Konten, Anti-Bot & Kriptografi Vault

1. **Zero-Localhost Secure Origin `nyari://` & Custom Link Triggers (`nyari-glance://`, `nyari-tab://`):**
   * UI disajikan dari memori (`include_dir!`) via `ICoreWebView2CustomSchemeRegistration` (`nyari://shell/`).
   * Saat registrasi `nyari`, **WAJIB** menyetel **`put_TreatAsSecure(TRUE)`**, **`put_HasAuthorityComponent(TRUE)`**, dan `SetAllowedOrigins(&["nyari://shell"])`.
   * Setiap *response* wajib menyertakan *CSP nonce* acak (`BCryptGenRandom`), `X-Frame-Options: DENY`, dan header `Access-Control-Allow-Origin: nyari://shell`. Matikan *Service Worker* pada skema `nyari://`.
   * Pada Content Controllers, `nyari-glance://open?url=...` dan `nyari-tab://open?url=...` wajib dicegat dan dibatalkan (`put_Cancel(TRUE)`) di `NavigationStarting`/`FrameNavigationStarting`, dan URL target yang di-decode **WAJIB** divalidasi hanya berskema `http://` atau `https://`.

2. **Isolasi Content Controller & Larangan Passive `Runtime.enable`:**
   * Seluruh *Content Controllers* **WAJIB** menyetel `put_IsWebMessageEnabled(FALSE)`, `put_AreHostObjectsAllowed(FALSE)`, dan `DefaultBackgroundColor.A = 255` (opaque), serta memblokir navigasi ke `nyari://`.
   * **DILARANG** memasang CDP `Runtime.enable` secara pasif pada Content Tabs (menghindari blokir anti-bot Cloudflare/DataDome/WAF).
   * Komunikasi balik & injeksi CDP hanya berjalan **secara on-demand**:
     - Untuk **Curtain** picker (`Ctrl+Shift+H` / `START_CURTAIN_PICKER`): aktifkan `Runtime.enable` sementara pada tab target -> `Page.createIsolatedWorld({ worldName: "nyari_curtain" })` -> `Runtime.addBinding({ name: "__nyari_bridge", executionContextName: "nyari_curtain" })`. Saat selesai/`Esc` (`STOP_CURTAIN_PICKER`), wajib evaluasi `delete globalThis.__nyari_bridge` di dalam `"nyari_curtain"`, panggil `Runtime.removeBinding({ name: "__nyari_bridge" })`, dan panggil `Runtime.disable`.
     - Untuk Vault autofill/capture, Reading Mode, pre-sleep snapshot, dan sesi MCP aktif: gunakan one-shot `Page.createIsolatedWorld` + `Runtime.callFunctionOn`/`Runtime.evaluate`, lalu segera panggil `Runtime.disable`.

3. **Kriptografi `vault.db` (Anti-Nonce-Reuse & Zero-Secret IPC) & Auth OS Universal Win10/11:**
   * Simpan 1 Master Key acak 32-byte (`BCryptGenRandom`, Base64-encoded 44 byte) di Windows Credential Manager via `keyring`.
   * Enkripsi kredensial sebagai **satu payload JSON (`payload_cipher`)** menggunakan **HKDF-SHA256** (16B `salt`) dan **AES-256-GCM** (12B `nonce` + **exact Web Origin `scheme://host[:port]` sebagai AAD**). `vault.db` wajib menegakkan `UNIQUE(origin_domain, username_hint)`.
   * **TIDAK ADA** hook capture pasif/otomatis di Content Tab: penangkapan sandi hanya melalui pre-submit **`VAULT_CAPTURE_FORM`** (Rust membungkus sandi dalam `Zeroizing<String>`, TTL 60s, di balik `promptId`) atau input manual **`VAULT_SAVE_MANUAL`** di `nyari://shell/`. **DILARANG** mengirim plaintext `password` dalam event menuju Shell UI.
   * Autofill & penghapusan kredensial wajib lolos *user gesture* + verifikasi OS:
     1. Utama (Win10 1903+/Win11): `UserConsentVerifier::CheckAvailabilityAsync()` + `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`.
     2. Fallback (`DeviceNotPresent`/`NotConfiguredForUser`): `CredUIPromptForWindowsCredentialsW(CREDUIWIN_GENERIC | CREDUIWIN_ENUMERATE_CURRENT_USER)` + validasi **`LsaConnectUntrusted` + `LsaLogonUser`** + verifikasi **`EqualSid(current_user_sid, logon_token_sid)`** pada `TokenUser` + `SecureZeroMemory`.
     3. Akun tanpa Windows Hello dan tanpa password (`STATUS_ACCOUNT_RESTRICTION`): *fail-closed* dengan prompt konfigurasi PIN/password Windows.
   * Injeksi autofill hanya diizinkan pada **top-level frame** (blokir cross-origin `<iframe>`). Setel `put_IsGeneralAutofillEnabled(FALSE)` dan `put_IsPasswordAutosaveEnabled(FALSE)` pada seluruh profil browsing.

4. **Keamanan Ekstensi CRX3 (`extensions.json` + Shim) & `nyari-mcp` Lockout:**
   * Instalasi `.crx` wajib memvalidasi header `Cr24\x03\x00\x00\x00`, mem-parse `CrxFileHeader`, menyuntikkan Base64 DER public key ke `"key"` di `manifest.json` bila kosong, menghapus folder/file top-level berawalan `_` kecuali `_locales/` (khususnya `_metadata/` yang ditolak `AddBrowserExtension` dengan `E_ACCESSDENIED`), menolak path traversal (`..`), dan memelihara **`extensions.json`** memakai `extension.Id()`. Ekstensi nonaktif pada profil `"nyari-private"`.
   * Pengaktifan `IsWebMessageEnabled(TRUE)` untuk shim `chrome.tabs` **HANYA** diperbolehkan pada controller dokumen `chrome-extension://{id}/` (`popup.html`/`options_page`, bukan MV3 Service Worker `#3852`), wajib memblokir navigasi keluar origin di `NavigationStarting`, memvalidasi `Source()` pada `WebMessageReceived`, menyuplai `window.__NYARI_ACTIVE_TAB__`, menangani `EXT_CREATE_TAB`, dan menyetel `put_Handled(TRUE)` pada `NewWindowRequested` ekstensi.
   * Server MCP (nonaktif default, tanpa port TCP) dikunci DACL SID pengguna (`\\.\pipe\NyariBrowser_MCP_<SID>`), mengekspos 7 tool terkurasi, memblokir akses ke `nyari://shell/`, Private Tabs, OAuth popups (`isOAuthPopup == true`), **menolak total `browser_eval_isolated` bila ada `<input type="password">` di halaman/frame target**, dan menyensor nilai password menjadi `"[REDACTED_PASSWORD]"` pada `browser_get_markdown_or_dom`.

---

## 3. Aturan Frontend Preact, Ad-Blocking & Cosmetic Injection

1. **Zero-Allocation `Shield`, Embedded Seed & YouTube JSON Ad Pruner:**
   * Seed filter default (~1.2 MB zstd/deflate) di-embed di `nyari.exe` untuk kompilasi `shield.bin` lokal tanpa request jaringan saat *first run*. Update jarak jauh hanya berjalan jika dipicu user (`UPDATE_SHIELD_LISTS`) atau `"shield_auto_update": true`.
   * Blokir jaringan `adblock-rust` wajib menggunakan **satu objek `ICoreWebView2WebResourceResponse` (`403 Forbidden`, `Content = None`) statis ter-cache** di memori STA.
   * Pemangkasan iklan JSON YouTube (`/youtubei/v1/`) dilakukan via skrip *main-world* prakompilasi (`AddScriptToExecuteOnDocumentCreated`) yang membungkus `Response.prototype.json` dan `JSON.parse` khusus domain `youtube.com`.

2. **Trusted Types, Reading Mode & InnerHTML Bypass:**
   * **DILARANG KERAS** menggunakan `element.innerHTML = ...` atau `document.write` dalam skrip injeksi Rust maupun Preact Shell UI (`Shield`, `Curtain`, *Isolated World*, *Reading Mode*).
   * Seluruh injeksi CSS cosmetic **WAJIB** menggunakan `document.adoptedStyleSheets` (via `CSSStyleSheet()`) atau API CDP `Page.addStyleSheet`. Reading Mode merender blok AST JSON murni via Preact VNodes di `nyari://shell/`.

3. **Omnibox Zero Keystroke Leak & FTS5 Query Sanitization:**
   * Pengisian karakter pada `CommandPalette.tsx` / `Omnibox` **DILARANG** memicu *network request* autocomplete ke server eksternal saat mengetik. Pencarian online hanya berjalan ketika tombol `Enter` ditekan.
   * Query SQLite FTS5 wajib di-escape (`"token"*`) sebelum dikirim ke `MATCH` agar karakter seperti `.`, `-`, atau `:` tidak menyebabkan syntax error.

4. **Reaktivitas UI via `@preact/signals`:**
   * Gunakan Signal untuk status reaktif UI (tab, workspace, memori, layout, zen mode, bookmarks, settings, MCP) untuk menghindari *unnecessary re-renders* DOM.

---

## 4. Checklist Pra-Commit

- [ ] Tidak ada pemanggilan `block_on` atau koneksi `rusqlite` langsung di UI Thread STA.
- [ ] Tidak ada penggunaan `ICoreWebView2Experimental*` / `CustomDataPartitionId`; Workspace memakai `"nyari-ws-{id}"`, Private Tab memakai `"nyari-private"` (`IsInPrivateModeEnabled = TRUE`).
- [ ] Seluruh overlay mengambang memakai Owned `WS_POPUP`/Popup Windows (`Layer 1` berbentuk-L `32 DIP` top caption di mode vertikal, `Layer 2` Priority Stack + Window-Family `WM_ACTIVATE` `GetAncestor` guard, OAuth Popup persisten); seluruh ukuran DIP dikalikan `dpi / 96.0`; Resize Border Ring disembunyikan saat maximized/fullscreen.
- [ ] Skema `nyari://` menyetel `TreatAsSecure(TRUE)` & `HasAuthorityComponent(TRUE)`; `nyari://`, `nyari-glance://`, & `nyari-tab://` tervalidasi ketat; `NewWindowRequested` memisahkan `isOAuthPopup` & mewarisi profil controller sumber.
- [ ] Content Controllers mengunci `IsWebMessageEnabled(FALSE)`, `AreHostObjectsAllowed(FALSE)`, dan **nol** `Runtime.enable` pasif (Curtain sementara: `Runtime.enable` -> `Page.createIsolatedWorld("nyari_curtain")` -> `Runtime.addBinding` -> `delete globalThis.__nyari_bridge` + `Runtime.removeBinding` + `Runtime.disable`).
- [ ] `vault.db` memakai `payload_cipher` tunggal (`HKDF-SHA256` + `AES-256-GCM` + exact origin AAD + `UNIQUE`), pre-submit `VAULT_CAPTURE_FORM` (`Zeroizing` + `promptId`), auth OS `UserConsentVerifier` / `CredUI` **`LsaLogonUser` + `EqualSid`**, dan injeksi top-frame only.
- [ ] Injeksi CSS/DOM bebas `innerHTML`; blokir `Shield` memakai static cached `403` & seed ter-embed; CRX3 strip `_metadata/` + `"key"` injection + `extensions.json` & `chrome.tabs` origin-locked shim (`EXT_CREATE_TAB`) hanya di dokumen `chrome-extension://`.
- [ ] `nyari-mcp` memblokir `nyari://`, Private Tabs, OAuth popups, menolak `browser_eval_isolated` pada halaman ber-`<input type="password">`, serta menyensor DOM password; `K32EmptyWorkingSet` abaikan `0x5`; tab `hasUnsavedInput == true` kebal Tier 2 Hard Unload.
- [ ] `WM_MOVE` memanggil `NotifyParentWindowPositionChanged`; `WM_CLOSE` memicu checkpoint SQLite Actor Thread dengan timeout ≤ 150 ms.
