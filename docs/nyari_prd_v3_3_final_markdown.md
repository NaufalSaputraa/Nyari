# Product Requirements & Technical Architecture Document — Nyari Browser

**Nama Produk:** Nyari Browser (`nyari.exe`)  
**Versi Dokumen:** v3.3 Final — *Zero-Gap Hardened Production Blueprint (50 Technical Mitigations)*  
**Target Platform:** Windows 10 (1903+, Build 18362+) & Windows 11 (`x86_64`)  
**Core Stack:** Rust (`tao` + `webview2-com = "0.39"` + `windows`) · Edge WebView2 · Preact + Signals · `adblock-rust` · `rusqlite` · `aes-gcm` + `hkdf` · `zeroize`  
**Lisensi & Prinsip:** MIT · *Local-First* · *True Zero-Telemetry* · *Ultra-Low Memory Footprint*

---

## 1. Ringkasan Eksekutif & Visi Produk

### 1.1 Latar Belakang Masalah
Browser modern berbasis Chromium mandiri (seperti Google Chrome dan Brave) mengalami pembengkakan konsumsi memori akibat tumpukan *background services*, *component updater*, sinkronisasi akun, serta alokasi *heap* V8 yang tidak agresif melepas memori ke sistem operasi. Pada skenario harian ringan (3 tab aktif), penggunaan RAM kerap menembus **1,2 – 1,5 GB**.

Di sisi lain, keputusan Kagi pada Oktober 2026 untuk menghentikan pengembangan Orion Browser (WebKit) di Windows dan Linux menegaskan bahwa memelihara *rendering engine* non-Chromium di Windows memakan biaya rekayasa terlalu tinggi dan mengorbankan kompatibilitas web modern.

### 1.2 Solusi & Sintesis Referensi
**Nyari** adalah browser *native* Windows yang dibangun dari nol (*clean-room architecture*) dengan menumpang runtime **Microsoft Edge WebView2** yang sudah tertanam di Windows 10/11, dikendalikan langsung oleh *backend* **Rust** untuk manajemen siklus memori tingkat OS dan isolasi keamanan ketat di atas **100% API WebView2 stabil (`webview2-com 0.39.1` / SDK `1.0.3800.47`)**.

Desain produknya memadukan elemen terbaik dari berbagai referensi open-source:
* **`driceroland/Search`:** Filosofi *local-first* tanpa akun, struktur kode modular *one-file-per-concern*, pemblokir elemen web permanen (*Curtain*), dan kontrol agen AI via MCP.
* **Zen Browser & `lumabrowser.dev`:** Antarmuka *vertical sidebar* yang dapat diciutkan (*collapse*), fleksibilitas pindah ke *top bar*, *Workspaces*, *Split View*, dan *Glance* (*link preview*).
* **Min Browser & Edge Internals:** Pembekuan tab otomatis yang ditingkatkan menjadi **3-Tier Memory Lifecycle** + **LRU Active Renderer Cap** + *BrowsingInstance* isolation (`nyari-tab://`) tanpa mematikan fitur keamanan *Site Isolation*.
* **`Magnetite`, `ilo`, & `devkecelup/kurowser`:** *Command Palette* serba guna (`Ctrl+K`), skema internal *zero-localhost* (`nyari://`) dengan *CSP nonce*, serta pruner iklan YouTube sisi klien.

### 1.3 Target Performa & Keamanan

| Metrik | Target Sistem Nyari | Pembanding (Brave / Chrome) |
| :--- | :--- | :--- |
| **Ukuran Installer / Binary** | **~8 – 15 MB** (termasuk UI bundle & seed filter `Shield`) | ~120 – 150 MB |
| **Startup ke Jendela Siap Pakai** | **Di bawah 500 ms** (profil `default` instan) | ~600 – 1.200 ms |
| **Cold Start RAM (1 tab ringan)** | **150 – 210 MB** | 350 – 500 MB |
| **Active Usage RAM (3–5 tab standar)** | **300 – 450 MB** | 1,1 – 1,5 GB |
| **Heavy Session (15 tab: 4 aktif + 11 *sleep/unload*)** | **450 – 650 MB** | 1,8 – 2,5 GB |
| **Pencarian History & Omnibox** | **Di bawah 1 ms** (SQLite FTS5) | ~10 – 50 ms |
| **Kompatibilitas Web & DRM** | **99,9%** (Zero passive `Runtime.enable`, Blink/V8 + Widevine DRM + *Site Isolation*; Passkey via `/open-in-edge`) | 99,9% |
| **Telemetri & Crash Reporting** | **0 request** (`IsCustomCrashReportingEnabled = true`, seed `Shield` ter-embed) | Aktif |

---

## 2. Spesifikasi Kebutuhan Fungsional (PRD)

### 2.1 Sistem Layout Adaptif (Zen Mode & Chrome Mode)
Pengguna dapat mengganti tata letak utama secara *real-time* lewat **Settings** atau pintasan `Ctrl+Shift+S` tanpa me-*reload* halaman web yang sedang terbuka. Lebar minimum jendela ditetapkan pada **330 DIP (logical pixels @ 96 DPI)** agar kompatibel penuh dengan seluruh zona *Windows 11 Snap Layouts*:

1. **Vertical Sidebar Mode (Default ala Zen Browser):**
   * **Expanded (`230 DIP`):** Menampilkan pemilih *Workspace*, *Pinned Tabs* (grid ikon kompak), daftar tab aktif dengan judul & indikator status memori, tombol ekstensi, bar caption atas `32 DIP` (`Layer 1` berbentuk-L via `SetWindowRgn` termasuk Window Controls & `Snap Overlay HWND` `46x32 DIP` di pojok kanan atas), serta pemicu *Command Palette*.
   * **Collapsed (`48 DIP`):** Mengecil menjadi rel ikon (*favicon*) vertikal dengan tetap mempertahankan bar caption atas `32 DIP` (*Layer 1* berbentuk-L agar tombol kontrol jendela & *Snap Overlay* tetap tidak tumpang tindih dengan kanvas web). Saat kursor diarahkan ke tepi kiri (*hover*), panel melebar sementara dalam **Owned Popup `HWND`** (`230 DIP`) di atas kanvas web tanpa memicu *resize* halaman di belakangnya.
   * **Zen Full Canvas (`0 DIP`, toggle `Ctrl+S`):** Menyembunyikan seluruh `Layer 1` dan *Snap Overlay HWND* (`SW_HIDE`) agar kanvas web tampil 100% penuh; navigasi & *Find-in-Page* (`Ctrl+F`) dibuka melalui `Layer 2`.
2. **Horizontal Top Bar Mode (Klasik ala Chrome):**
   * Memindahkan deretan tab ke sisi atas jendela secara horizontal dengan satu baris *omnibox* tipis di bawahnya (`42 DIP`).

### 2.2 Unified Command Palette & Omnibox (`Ctrl+K` / `Ctrl+L`)
* **Satu Kolom Terpadu:** Berfungsi sekaligus sebagai *address bar*, pencari tab terbuka (*tab switcher*), pencari riwayat dan *bookmark* lokal (SQLite FTS5 dengan sanitasi token kueri), peluncur perintah browser (`/split`, `/layout`, `/clear-cache`, `/open-in-edge`), dan pencarian web.
* **Zero Keystroke Leak:** Tidak mengirim karakter ketikan ke server mesin pencari saat pengguna mengetik; *request* jaringan hanya dikirim setelah tombol `Enter` ditekan.
* **Custom Search Engine:** Mendukung pemilihan mesin pencari default (DuckDuckGo, Google, Brave Search, Startpage) maupun *custom URL template* (`https://...?q=%s`).

### 2.3 Manajemen Siklus Tab, Workspaces, Split View, Glance & Popup OAuth
* **True Lazy-Loading saat Startup:** Tab dari sesi terakhir hanya dimuat sebagai *struct* metadata di Rust. *Controller* WebView2 baru diinstansiasi saat tab tersebut diklik.
* **Keep Site Awake:** Opsi klik kanan pada tab untuk mengecualikan situs penting (WhatsApp Web, pemutar musik, *dashboard* pemantauan) dari pembekuan memori otomatis.
* **Workspaces & Isolasi Profil Stabil (`nyari-ws-{id}`):** Pengelompokan tab berdasarkan konteks kerja (*Utama*, *Kerja*, *Riset*). Setiap Workspace memakai **Named Profile terisolasi (`nyari-ws-{id}`)** melalui API stabil `ICoreWebView2ControllerOptions::put_ProfileName` (isolasi penuh *cookie*, LocalStorage, IndexedDB, Service Worker, dan HTTP Cache). Profil Workspace aktif diinisialisasi secara asinkron tepat setelah *Shell UI* tampil; Workspace lain diinisialisasi secara *lazy* saat pertama dibuka. Saat Workspace dihapus, Nyari menutup seluruh tabnya lalu memanggil **`ICoreWebView2Profile8::Delete()`**.
* **Pemisahan `BrowsingInstance` via `nyari-tab://` untuk Middle-Click / `Ctrl+Klik`:** Karena `NewWindowRequested` tidak membedakan klik tengah/`Ctrl+Klik` dari `window.open()`, skrip `AddScriptToExecuteOnDocumentCreated` mencegat `auxclick` (`button === 1`) dan `click` (`ctrlKey`/`metaKey` tanpa `altKey`) lalu mengarahkannya ke `nyari-tab://open?url=...` (dibatalkan di `NavigationStarting`, divalidasi `http`/`https`) untuk membuka controller independen pada profil yang sama — memutus `BrowsingInstance` agar `TrySuspend` berjalan andal.
* **Split View (`Alt+Ctrl+N`):** Menjejarkan dua halaman web berdampingan dalam satu jendela dengan *divider* yang dapat digeser ukurannya secara serempak dan bebas kedipan.
* **Glance (`Alt+Klik` pada tautan biasa):** Membuka pratinjau tautan eksternal di **Owned Popup `HWND`** terpusat (~80% ukuran *viewport*). Penangkapan `Alt+Klik` mengarah ke `nyari-glance://open?url=...` (dibatalkan di `NavigationStarting` dengan validasi skema `http`/`https`) — **tanpa** CDP `Runtime.enable`. Dapat ditutup seketika dengan `Esc` atau klik area kanvas utama, maupun dipromosikan menjadi tab reguler.
* **Kompatibilitas Penuh Popup OAuth & `window.open()`:** `NewWindowRequested` memeriksa `args.WindowFeatures()` (`HasSize` / `HasPosition` / chrome flags): jika merupakan popup OAuth (`is_popup == true`), controller dibuka di **Top-Level Owned OAuth Popup Window** terpisah (tidak menutup saat `WM_ACTIVATE(WA_INACTIVE)` sehingga pengguna bisa menyalin kode 2FA, dan menandai `isOAuthPopup: true`); jika `<a target="_blank">` biasa (`is_popup == false`), dibuka sebagai tab reguler di `Content Host HWND`. Keduanya memakai *fast-path deferral* (< 5 ms setup sinkron STA, < 150 ms async) dengan **mewarisi profil controller sumber (`nyari-ws-{id}` atau `"nyari-private"`)**.
* **Kelengkapan Fitur Harian Browser:**
  * **Find-in-Page (`Ctrl+F`):** Dihosting secara seragam di **`Layer 2` (`Shell Overlay Popup`, Priority 2, sudut kanan atas kanvas web)** pada seluruh mode layout menggunakan API natif `ICoreWebView2_28::Find` (WebView2 Runtime 139+), dengan *graceful fallback* pada runtime lebih lama.
  * **HTML5 Fullscreen & Picture-in-Picture (`Ctrl+Shift+P`):** Fullscreen menyembunyikan Shell UI dan *Resize Border Ring*; keluar PiP wajib memanggil `document.exitPictureInPicture()`.
  * **Download, Permission, Screen Capture, Script Dialog, Basic Auth & External URI Manager:** Panel unduhan kustom terintegrasi + kartu dialog UI per domain (`PermissionRequested`, `ScreenCaptureStarting`, `ScriptDialogOpening`, `BasicAuthenticationRequested`, `LaunchingExternalUriScheme`).
  * **Passkey Escape Hatch (`/open-in-edge`):** Meneruskan URL halaman aktif ke Microsoft Edge sistem.
  * **Reading Mode (`Ctrl+Shift+R`):** Mengekstrak artikel terstruktur (`{ title, byline, blocks }`) melalui satu kali panggilan *one-shot* CDP Isolated World lalu merendernya di `nyari://shell/` memakai Preact VNode (kebal *Trusted Types*).

### 2.4 Kontrol Konten & Privasi Jaringan (`Shield`, `Curtain` & `Private Tab`)
* **Shield (Network-Level Ad & Tracker Blocker):** Mem-embed *seed filter list* terkompresi (~1.2 MB EasyList + EasyPrivacy + Peter Lowe's AdServers) di dalam `nyari.exe` untuk kompilasi `shield.bin` lokal tanpa request jaringan saat *first run* (update jarak jauh bersifat manual/opt-in). Mencegat request via `adblock-rust` (`Arc<Engine>`) dengan **satu objek `ICoreWebView2WebResourceResponse` (`403 Forbidden`) statis ter-cache** (*zero-allocation*), dilengkapi *cosmetic CSS* via `adoptedStyleSheets` dan pruner JSON YouTube sisi klien (`AddScriptToExecuteOnDocumentCreated` pada host `youtube.com`).
* **Curtain (Persistent Element Hider):** Mode inspeksi (`Ctrl+Shift+H`) mengaktifkan `Runtime.enable` **sementara pada tab target saja** lalu `Page.createIsolatedWorld` (`worldName: "nyari_curtain"`) + `Runtime.addBinding({ name: "__nyari_bridge", executionContextName: "nyari_curtain" })` selama sesi pemilihan elemen. Saat elemen dipilih atau `Esc` ditekan, Rust menghapus `globalThis.__nyari_bridge`, memanggil `Runtime.removeBinding`, dan segera menutup dengan `Runtime.disable`. Selector CSS disimpan di `curtain.json` dan disuntikkan via `CSSStyleSheet()` di `AddScriptToExecuteOnDocumentCreated`.
* **Private Tab (`Ctrl+Shift+N`):** Menggunakan profil terpisah **`ProfileName = "nyari-private"`** dengan **`put_IsInPrivateModeEnabled(TRUE)`** — terisolasi penuh dari penghapusan Workspace, menonaktifkan ekstensi Workspace, dan **tidak pernah** dicatat ke `session.json` atau `history.db`.

### 2.5 Penyimpanan Lokal, Vault Terenkripsi & Ekstensi Chrome
* **Local-First Storage:** Seluruh data disimpan lokal di `%LOCALAPPDATA%\NyariBrowser\` tanpa akun dan tanpa *cloud sync*.
* **Encrypted Credential Vault + Autentikasi OS Universal Win10/11:** Kredensial disimpan sebagai payload JSON tunggal terenkripsi (`payload_cipher`) di `vault.db` menggunakan **HKDF-SHA256** (16B `salt`) + **AES-256-GCM** (12B `nonce` + **exact Web Origin `scheme://host[:port]` sebagai AAD**), dengan constraint `UNIQUE(origin_domain, username_hint)`. Master Key 32-byte (`BCryptGenRandom`, Base64 44 byte) disimpan di Windows Credential Manager (`keyring`).
  * **Pre-Submit On-Demand Capture & Manual Entry (Zero Secret Leak):** Tanpa hook pasif di Content Tab, penyimpanan kredensial dilakukan melalui **Pre-Submit Capture (`VAULT_CAPTURE_FORM { tabId }` sebelum klik Submit)** yang membungkus sandi di Rust (`Zeroizing<String>`, TTL 60 detik) di balik `promptId` -> `VAULT_PROMPT_SAVE` -> `VAULT_CONFIRM_SAVE`, atau input manual `VAULT_SAVE_MANUAL` di `nyari://shell/`.
  * **Autofill Availability, Top-Frame Injection & OS Verification:** Pada `NavigationCompleted`, Rust mengecek exact Web Origin di `vault.db` dan memancarkan `VAULT_AUTOFILL_AVAILABLE`. Autofill (`VAULT_UNLOCK_AND_FILL`) dan hapus kredensial wajib lolos **OS Auth** (Utama Win10 1903+/Win11: `UserConsentVerifier::CheckAvailabilityAsync` + `IUserConsentVerifierInterop`; Fallback tanpa Windows Hello: `CredUIPromptForWindowsCredentialsW(CREDUIWIN_GENERIC | CREDUIWIN_ENUMERATE_CURRENT_USER)` + validasi **`LsaConnectUntrusted` + `LsaLogonUser`** + verifikasi **`EqualSid(current_user_sid, logon_token_sid)`** + `SecureZeroMemory`; *fail-closed* bila `STATUS_ACCOUNT_RESTRICTION`/tanpa PIN/sandi), lalu menyuntikkan nilai ke **top-level frame saja** via *one-shot* CDP Isolated World. Mode autofill Edge bawaan dinonaktifkan total.
* **Ekstensi Chrome (`ICoreWebView2Profile7` + `extensions.json` + `chrome.tabs` Popup Shim):**
  * Mengaktifkan `ICoreWebView2EnvironmentOptions6::put_AreBrowserExtensionsEnabled(TRUE)`.
  * Mendukung folder *unpacked* maupun `.crx` (CRX3: validasi magic `Cr24\x03\x00\x00\x00`, ekstraksi DER public key dari `CrxFileHeader` ke `"key"` di `manifest.json` bila kosong agar ID kanonik 32-karakter `a`–`p` cocok, menghapus folder reserved `_metadata/` yang memicu `E_ACCESSDENIED` pada `AddBrowserExtension`, ekstraksi ZIP aman ke `Extensions\{id}\{version}\`, dan persistensi **`extensions.json`** menggunakan `extension.Id()`).
  * **Popup, Security Lock & Batas MV3 Service Worker:** *Action popup* (`chrome-extension://{id}/{default_popup}`) dan `options_page` dirender pada **profil Workspace aktif (`nyari-ws-{id}`)** di dalam **Owned Popup `HWND`** (`200x150`–`800x600 DIP`). Khusus controller dokumen `chrome-extension://`, `IsWebMessageEnabled(TRUE)` diizinkan dengan mengunci `NavigationStarting` agar tidak keluar dari `chrome-extension://{id}/` serta memvalidasi `Source()` pada `WebMessageReceived`. Rust menyuntikkan `window.__NYARI_ACTIVE_TAB__` secara sinkron untuk `chrome.tabs.query` dan menerima `{ type: "EXT_CREATE_TAB", url }` dari `chrome.tabs.create` (+ `NewWindowRequested` `put_Handled(TRUE)`). Shim ini hanya berlaku pada dokumen (`popup.html`/`options_page`) dan tidak berjalan di MV3 background Service Worker (`WebView2Feedback #3852`); ekstensi nonaktif di Private Tab.

### 2.6 Kontrol Agen AI via MCP (`nyari-mcp` / `nyari.exe --mcp-stdio`)
* Server MCP (nonaktif secara default) mendengarkan pada **`\\.\pipe\NyariBrowser_MCP_<USER_SID>`** (DACL terkunci SID pengguna, framing JSON-RPC 2.0 dipisah `\n`), tanpa port TCP `--remote-debugging-port`.
* Mode **`nyari.exe --mcp-stdio`** bertindak sebagai *headless stdio-to-Named-Pipe proxy* (< 2 MB RSS) bagi host MCP standar (Claude Desktop, Cursor, IDE) tanpa mengambil mutex UI.
* Mengekspos **7 tool terkurasi** (`browser_list_tabs`, `browser_navigate`, `browser_get_markdown_or_dom`, `browser_click`, `browser_type`, `browser_screenshot`, `browser_eval_isolated`) dengan **hard blocklist**: menolak `nyari://shell/`, Private Tabs, OAuth popups, menolak total `browser_eval_isolated` jika halaman/frame mengandung `<input type="password">`, serta menyensor nilai field sandi menjadi `"[REDACTED_PASSWORD]"` pada `browser_get_markdown_or_dom`.
* Mendukung persetujuan bertingkat (`allow_once` | `allow_origin_session` | `deny`) serta pil **Stop** yang selalu terlihat di Shell UI.

---

## 3. Arsitektur Teknis & Desain Sistem (*Zero-Gap Hardened*)

### 3.1 Tech Stack & Crate Utama (Rust)

| Lapisan | Teknologi / Crate | Peran Teknis |
| :--- | :--- | :--- |
| **Windowing & OS Native** | `tao` + `windows` (Win32, DWM, WinRT, Security, LSA) | *Frameless window* (`WS_EX_NOREDIRECTIONBITMAP`), Per-Monitor DPI V2 (`WM_DPICHANGED`), *L-Shaped `Layer 1` & Owned `WS_POPUP` HWNDs*, *Dynamic Resize Border Ring `HWND`*, `WM_NCCALCSIZE`, `WM_ERASEBKGND`, *Snap Overlay HWND*, Single-Instance Mutex + DACL Named Pipe, Windows Hello & `LsaLogonUser` Interop. |
| **WebView COM Binding** | `webview2-com = "0.39"` (0.39.1) | API stabil: `ICoreWebView2Environment13`, `EnvironmentOptions6`, `ICoreWebView2Controller3`, `ControllerOptions`, `ICoreWebView2Profile7`/`8`, `ICoreWebView2_22`, `ICoreWebView2_28`, & CDP Session API. |
| **Shell UI Frontend** | **Preact + `@preact/signals` + CSS** (`include_dir!`) | UI reaktif ultra-ringan (~15 KB *bundle* di-embed di binary, tanpa *Service Worker*), pembaruan DOM atomik, & animasi 120fps. |
| **Ad-Blocker Engine** | `adblock` (`brave/adblock-rust`) | Pencocokan aturan blokir iklan *lock-free* (`Arc<adblock::Engine>`), seed terkompresi ~1.2 MB di binary, & static cached `403` response. |
| **Database & Crypto** | `rusqlite` (WAL + FTS5 actor thread), `aes-gcm`, `hkdf`, `sha2`, `keyring`, `zeroize` | Pencarian *history/bookmark* < 1 ms (tanpa `SQLITE_BUSY`) + Vault terenkripsi (`HKDF-SHA256` + `AES-256-GCM` + AAD; Master Key 32-byte di OS). |
| **Async & IPC** | `tokio` + `EventLoopProxy` | Penjadwal *memory reclaimer* dan server *Named Pipe* (Single-Instance & MCP) satu arah ke COM STA UI Thread. |

### 3.2 Startup Sequence, Single-Instance Lock & Profil Asinkron
1. **Flag `--mcp-stdio`:** Jika dijalankan dengan `--mcp-stdio`, lewati inisialisasi UI/mutex dan langsung jalankan loop relay `stdin`/`stdout` ↔ `\\.\pipe\NyariBrowser_MCP_<USER_SID>`.
2. **Single-Instance Guard (`Local\NyariBrowser_Instance`) + Anti-Race Named Pipe:**
   * Proses utama segera membuka server *Named Pipe* (`\\.\pipe\NyariBrowser_Instance_<SID>`, DACL SID pengguna) **sebelum** membuat jendela atau WebView2.
   * Instans kedua yang mendapati `ERROR_ALREADY_EXISTS` memanggil `WaitNamedPipeW` (retry 1 ms, maks 500 ms), mengirim argumen, dan `std::process::exit(0)` dalam **< 10 ms** — mencegah error `0x8007139F`.
3. **Deteksi WebView2 Runtime Hilang / Versi Baru:** Dialog ramah ke *Evergreen Bootstrapper* jika runtime tidak ada, plus `add_NewBrowserVersionAvailable` untuk pil notifikasi update di Sidebar.
4. **Arsitektur Profil Stabil Asinkron (Fix #14, #25, #45):**
   * *Environment* dibuat dengan **`ExclusiveUserDataFolderAccess = true`**, **`IsCustomCrashReportingEnabled = true`**, dan **`AreBrowserExtensionsEnabled = true`**.
   * Shell UI berjalan di profil `default` (`ProfileName = nullptr`, `DefaultBackgroundColor.A = 0`).
   * Tab reguler berjalan di Named Profile per Workspace (`nyari-ws-{id}`) asinkron/lazy; Private Tab di `"nyari-private"` (`IsInPrivateModeEnabled = TRUE`).

### 3.3 Model Threading: Strict COM STA, Tokio & Dedicated SQLite Actor
* **Main Thread (Win32 Message Loop + COM STA):** Pemilik tunggal seluruh `ICoreWebView2*` dan `HWND`. Pemanggilan `block_on` di UI thread dilarang keras.
* **Tokio Background Runtime:** Menangani timer siklus memori, I/O *Named Pipe* (Single-Instance & MCP), dan *polling* statistik sistem via satu arah `EventLoopProxy`.
* **Dedicated SQLite Actor Thread:** Satu OS thread khusus memiliki koneksi `rusqlite::Connection` untuk `history.db` dan `vault.db` via `std::sync::mpsc::sync_channel`.

### 3.4 Arsitektur 2-Layer Shell, Owned `WS_POPUP` Priority Stack & Window-Family `WM_ACTIVATE`
Komposisi jendela memisahkan pane `WS_CHILD` non-tumpang-tindih dari surface overlay `WS_POPUP` milik `main_hwnd`:

```text
+-------------------------------------------------------------------------+
|  Main Win32 Window (tao + WS_EX_NOREDIRECTIONBITMAP + WM_NCCALCSIZE)    |
|  Per-Monitor DPI V2 (WM_DPICHANGED -> Scale = dpi / 96.0)               |
|  WM_ERASEBKGND -> 1 | WM_WINDOWPOSCHANGED -> Sync Child & Popup HWNDs   |
|  Nyari Window-Family WM_ACTIVATE Guard (GetAncestor GA_ROOT)            |
|                                                                         |
|  [Dynamic Resize Border Ring HWND] (Edge 6 DIP * scale, SetWindowRgn)   |
|  -> Recalculate HRGN on resize; SW_HIDE saat Maximized / Fullscreen     |
|                                                                         |
|  [Native Win32 Snap Overlay HWND] -> di atas Maximize (46x32 DIP)       |
|  (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS, NULL_BRUSH -> HTMAXBUTTON)   |
|                                                                         |
|  +------------------+  +---------------------------------------------+  |
|  | [Layer 1]        |  | [Content Layer] Content Host HWND(s)        |  |
|  | Base Chrome HWND |  | Profile: "nyari-ws-{id}" / "nyari-private"  |  |
|  | (WS_CHILD)       |  | (WS_CHILD, A=255 Opaque)                    |  |
|  | Profile: Default |  | - NavigationStarting nyari:// Blocked       |  |
|  | (A=0 Mica)       |  | - WebMessage & HostObjects = FALSE          |  |
|  +------------------+  | - Zero passive Runtime.enable               |  |
|                        +---------------------------------------------+  |
+-------------------------------------------------------------------------+
       |
       +-- Owned Top-Level Windows (hwndParent = main_hwnd):
             1. Shell Overlay Popup (Layer 2, WS_POPUP, Default, nyari://shell/)
                -> Single-Surface Priority Stack (P1 Modal FIFO -> P2 Palette/FindInPage
                   -> P3 Reading Mode -> P4 Hover-Sidebar Drawer) via SET_OVERLAY_BOUNDS
             2. Glance Popup (WS_POPUP, Active Workspace / "nyari-private")
                -> Floating ~80% Viewport Link Preview (tutup saat WA_INACTIVE ke main)
             3. Extension Action Popup (WS_POPUP, Active Workspace)
                -> chrome-extension://{id}/{popup} + WebMessage locked chrome.tabs shim
             4. OAuth / Sized Popup Window (WS_OVERLAPPEDWINDOW / framed WS_POPUP)
                -> isOAuthPopup: true; TETAP TERBUKA saat WA_INACTIVE (copy 2FA)
```

* **Per-Monitor DPI V2 & Anti-Flicker:** Seluruh ukuran **DIP** dikalikan `GetDpiForWindow(hwnd) as f64 / 96.0` pada Win32/`put_Bounds()`. `WM_DPICHANGED` menyelaraskan posisi dan `put_RasterizationScale`. `WM_NCCALCSIZE` menjepit ke `MonitorInfo.rcWork`; Win10 memanggil `DwmExtendFrameIntoClientArea` (`cyTopHeight: 1`). Di mode vertikal, `Layer 1` membentuk geometri berbentuk-L (`32 DIP` top caption strip + left sidebar via `SetWindowRgn`) agar tombol Window Controls & `Snap Overlay HWND` tetap non-tumpang-tindih di pojok kanan atas.
* **Window-Family `WM_ACTIVATE` Guard (`#47`):** Klik di dalam salah satu owned popup tidak menutup popup (`GetAncestor(lparam, GA_ROOT)` termasuk anggota keluarga Nyari). Klik kembali ke `main_hwnd` / `Content Host` menutup popup transien (`CommandPalette`, `Hover-Sidebar`, `Extension Action Popup`, `Glance`) tetapi tetap mempertahankan Priority 1 Modal Dialogs, Reading Mode, dan **OAuth Popup Windows (`#5`)** hingga diselesaikan atau ditutup lewat `Esc`/`window.close()`. Perpindahan ke aplikasi lain (`Alt+Tab`) menyembunyikan popup transien.
* **Dynamic `SetWindowRgn` Ring:** Pada `WM_WINDOWPOSCHANGED`, sembunyikan Resize Border Ring (`SW_HIDE`) saat `Maximized`/`Fullscreen`; saat `Restored`, hitung ulang `CombineRgn(RGN_DIFF)` dan terapkan `SetWindowRgn`.
* **Routing Shortcut & Fokus:** Filter `KeyEventKind == KeyDown | SystemKeyDown`, `put_IsBrowserAcceleratorKeyEnabled(FALSE)`, kirim `SHORTCUT_TRIGGERED` ke Shell UI, dan panggil `MoveFocus(PROGRAMMATIC)` ke tab aktif saat Overlay Popup ditutup.

### 3.5 Manajemen Siklus Web, Fast-Path OAuth, Crash Recovery & Shutdown
1. **Sequenced `NewWindowRequested`, Discriminator Popup & Source-Profile Inheritance:** `GetDeferral()` (< 5 ms sync STA setup), buat controller pada **profil yang sama dengan controller sumber (`nyari-ws-{id}` atau `"nyari-private"`)** (`A=255`), periksa `WindowFeatures` (`HasSize`/`HasPosition`/chrome flags) untuk memisahkan **OAuth Popup Window (`isOAuthPopup: true`)** vs tab reguler (`isOAuthPopup: false`), pasang `IsWebMessageEnabled(FALSE)`, `AreHostObjectsAllowed(FALSE)`, guard `nyari://`, `nyari-glance://`, & `nyari-tab://`, daftarkan skrip pre-compiled via `AddScriptToExecuteOnDocumentCreated`, selesaikan `put_NewWindow()` + `deferral.Complete()` (< 150 ms end-to-end). Daftarkan `WindowCloseRequested`.
2. **Event Inti Browser:** `NavigationCompleted` (cek origin `vault.db` → `VAULT_AUTOFILL_AVAILABLE`), `ContainsFullScreenElementChanged`, `DownloadStarting`, `PermissionRequested`, `ScreenCaptureStarting`, `ScriptDialogOpening`, `BasicAuthenticationRequested`, `LaunchingExternalUriScheme`.
3. **Resiliensi & Shutdown:** Tunggu `BrowserProcessExited` sebelum membuat ulang environment. Pada `WM_CLOSE`: tulis atomik `session.json.tmp → rename` (tanpa Private Tabs), kirim `ShutdownAndCheckpoint` ke **SQLite Actor Thread** (timeout 150 ms untuk `PRAGMA wal_checkpoint(TRUNCATE)`), hapus buffer pending Vault (`Zeroizing`), dan `Close()` seluruh controller.

### 3.6 Mesin Efisiensi Memori: *3-Tier Lifecycle* + *Dirty-Form Guard*

| Mekanisme / Tier | Kondisi Pemicu | Implementasi Teknis (Rust + Win32 + WebView2) | Dampak RAM & Stabilitas |
| :--- | :--- | :--- | :--- |
| **V8 Heap Profile** | Inisialisasi Environment | `--js-flags=--max-old-space-size={val}` (**Eco 512MB**, **Balanced 1024MB**, **Unlimited**). | Mencegah OOM crash di web berat (Figma/Canva). |
| **LRU Active Renderer Cap** | Tab aktif > cap (default: 4–5) | Tab aktif terlama didorong ke Tier 1 soft sleep tanpa menunggu timer 3 menit. | Menjaga jumlah proses *renderer* aktif tetap terkendali. |
| **Tier 1: Soft Sleep** | Tab idle > 3 menit / window `SIZE_MINIMIZED` | 1. **Pre-Sleep Snapshot (One-Shot CDP):** Ambil `scrollX`, `scrollY`, & `hasUnsavedInput` sebelum tidur lalu `Runtime.disable`.<br>2. Validasi `IsVisible == false` dan tab tidak memutar audio.<br>3. Panggil `TrySuspend()` & periksa `isSuccessful`:<br>   - `true`: `FrameId → PID` (`GetProcessExtendedInfos`) → *Shared-PID Guard* → `K32EmptyWorkingSet` (abaikan `0x5`).<br>   - `false`: tandai `soft_sleep_degraded`, terapkan `MemoryUsageTargetLevel::LOW` + best-effort trim. | RAM fisik turun **60–80%** (sisa ~20–35 MB/tab). Bangun instan (< 50 ms) tanpa *reload*. |
| **Tier 2: Hard Unload** | Tab idle > 25 menit (kecuali Pinned, KeepAwake, atau **`hasUnsavedInput == true`**) | Simpan metadata & posisi scroll pre-sleep ke `session.json`, lalu panggil `Close()` pada controller tanpa membangunkan renderer. | Konsumsi RAM **0 MB**. Draft formulir tidak pernah hilang. |

### 3.7 Strategi *Network Ad-Blocking*, Static 403 Cache & Skema `nyari://`
* **Custom Scheme Registration (`#34`):** Skema `nyari` didaftarkan dengan **`put_TreatAsSecure(TRUE)`**, **`put_HasAuthorityComponent(TRUE)`**, dan `SetAllowedOrigins(&["nyari://shell"])`.
* **API Modern (`ICoreWebView2_22`), Embedded Seed (`#44`) & Static 403 (`#35`):** `AddWebResourceRequestedFilterWithRequestSourceKinds` (`SOURCE_KINDS_ALL`, disaring ke `SCRIPT`, `SUBDOCUMENT`, `XHR`, `FETCH`). Seed ~1.2 MB di-compile ke `shield.bin` lokal saat *first run*; blokir iklan memakai **satu objek `403 Forbidden` statis ter-cache**.
* **Resolver `source_url` Berjenjang & Trusted Types:** Header `Origin`/`Referer` → cache `NavigationStarting` → bypass `$third-party` jika kosong. CSS disuntikkan via `document.adoptedStyleSheets` / CDP `Page.addStyleSheet`; YouTube JSON pruner berjalan di *main world* (`youtube.com`).

### 3.8 Arsitektur Penyimpanan Data Lokal (`%LOCALAPPDATA%\NyariBrowser\`)
* **`settings.json`:** Preferensi layout (`layout_mode`, `zen_mode`), V8 Heap, *LRU Cap*, durasi sleep/unload, tema, search engine, `shield_auto_update` (default `false`), `mcp_enabled` (default `false`).
* **`session.json`:** Workspaces, urutan tab, `scroll_x`/`scroll_y`, status *Pinned/KeepAwake*, dan *Split View* (atomik `.tmp`; Private Tabs dikecualikan).
* **`extensions.json`:** Registry metadata ekstensi per Workspace (`id`, `workspaceId`, `name`, `version`, `enabled`, `folderPath`, `defaultPopup`, `optionsPage`, `iconDataUrl`).
* **`curtain.json`:** Selector CSS element hider per domain.
* **`history.db` (SQLite WAL + FTS5 via Actor Thread):** `visits` (`workspace_id` + `UNIQUE(workspace_id, url)` upsert) dan `bookmarks`.
* **`vault.db` (HKDF-SHA256 + AES-256-GCM + AAD via Actor Thread):** `credentials` (`UNIQUE(origin_domain, username_hint)`, `payload_cipher`, 16B `salt`, 12B `nonce`).
* **`shield.bin`:** Serialized binary engine `adblock-rust` (dikompilasi dari seed ter-embed).
* **`Extensions\` & `EBWebView\`:** Extracted unpacked extensions & profil Workspace (`nyari-ws-{id}`).

---

## 4. Arsitektur Keamanan & Pagar Pengaman (50 Mitigasi Lengkap)

1. **Zero-Localhost `nyari://` Secure Origin + Owned Popup Isolation:**
   * UI disajikan dari memori (`include_dir!`) pada `nyari://shell/` (`TreatAsSecure = TRUE`, `HasAuthorityComponent = TRUE`, *CSP nonce* `BCryptGenRandom`, `Access-Control-Allow-Origin: nyari://shell`, `X-Frame-Options: DENY`).
   * Seluruh *Content Controllers* mengunci **`put_IsWebMessageEnabled(FALSE)`**, **`put_AreHostObjectsAllowed(FALSE)`**, memblokir navigasi ke `nyari://`, `nyari-glance://`, dan `nyari-tab://` (hanya target `http://`/`https://`), dan **tidak pernah** mengaktifkan `Runtime.enable` secara pasif.
2. **Zero-Telemetry & SSL:** Validasi TLS/SSL ketat, `IsCustomCrashReportingEnabled = true`, seed `Shield` offline ter-embed.
3. **Autentikasi Vault OS, Anti-Nonce-Reuse & Zero-Secret IPC:** Master Key 32B (Base64) di OS; `HKDF-SHA256` + `AES-256-GCM` (`origin_domain` AAD, `payload_cipher` tunggal, `UNIQUE(origin_domain, username_hint)`); pre-submit buffer capture (`Zeroizing<String>`, `promptId`); auth via `UserConsentVerifier` + fallback `CredUI` (`CREDUIWIN_ENUMERATE_CURRENT_USER`) **`LsaLogonUser` + `EqualSid`** (*fail-closed* bila `STATUS_ACCOUNT_RESTRICTION` / tanpa PIN/sandi); autofill edge mati; injeksi top-frame only via one-shot CDP Isolated World.
4. **Named Pipe DACL & MCP Password Lockout:** Pipe Single-Instance & MCP dikunci DACL SID pengguna; `--mcp-stdio` tanpa port TCP; blocklist terhadap `nyari://shell/`, Private Tabs, OAuth popups, penolakan total `browser_eval_isolated` di halaman dengan `<input type="password">`, serta redaksi nilai password pada `browser_get_markdown_or_dom`.

---

## 5. Sistem Desain Visual, Integrasi Win32 DWM & *Design Tokens*

### 5.1 Design Tokens (CSS Variables)

```css
:root {
  --font-ui: "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
  --font-mono: "Cascadia Code", "JetBrains Mono", Consolas, monospace;
  --radius-window: 10px;
  --radius-canvas: 8px;
  --radius-item: 7px;
  --sidebar-expanded: 230px;
  --sidebar-collapsed: 48px;
  --topbar-height: 42px;
  --ease-spring: cubic-bezier(0.2, 0.9, 0.3, 1.0);
}

[data-theme="dark"] {
  --bg-frame: rgba(7, 11, 25, 0.84);
  --bg-frame-solid: #070B19;
  --bg-surface: #0E152B;
  --bg-surface-elevated: #151F3D;
  --accent-primary: #6366F1;
  --accent-hover: #818CF8;
  --text-primary: #F1F5F9;
  --text-secondary: #94A3B8;
  --status-sleep: #38BDF8;
}

[data-theme="light"] {
  --bg-frame: rgba(241, 245, 249, 0.85);
  --bg-frame-solid: #F1F5F9;
  --bg-surface: #F8FAFC;
  --bg-surface-elevated: #E2E8F0;
  --accent-primary: #4F46E5;
  --accent-hover: #4338CA;
  --text-primary: #0F172A;
  --text-secondary: #475569;
  --status-sleep: #0284C7;
}
```

### 5.2 Pintasan Keyboard Utama

| Pintasan | Aksi |
| :--- | :--- |
| `Ctrl + K` / `Ctrl + L` | Buka *Owned Popup Command Palette* / fokus *Omnibox* |
| `Ctrl + F` | Buka *Find-in-Page* natif (`ICoreWebView2_28::Find` atau fallback) |
| `Ctrl + T` / `Ctrl + W` | Tab baru / tutup tab aktif |
| `Ctrl + Shift + N` | Buka *Private Tab* (`nyari-private`, `IsInPrivateModeEnabled = TRUE`) |
| `Ctrl + S` / `Ctrl + Shift + S` | Toggle Zen Mode / Switch Layout (Sidebar <-> TopBar) |
| `Alt + Ctrl + N` | Toggle *Split View* berdampingan |
| `Alt + Klik Link` | Buka pratinjau tautan di *Owned Popup* *Glance* |
| `Ctrl + Shift + H` | Inspection mode *Curtain* (on-demand CDP Isolated World) |
| `Ctrl + Shift + R` | Toggle *Reading Mode* (one-shot Readability -> `nyari://shell/`) |
| `Ctrl + Shift + P` | Toggle *Picture-in-Picture* |

---

## 6. Matriks Mitigasi Lengkap (50 Celah Audit Teknis)

| # | Celah Teknis & Sumber Audit | Solusi Final di Arsitektur Nyari v3.3 | Modul Target |
| :--- | :--- | :--- | :--- |
| **1** | **`WM_NCHITTEST` Induk Gagal Memicu *Snap Layouts*** (`Zbrooklyn/tauri-snap-layouts`) | Pasang *native Win32 child window* (`WS_CHILD \| WS_VISIBLE \| WS_CLIPSIBLINGS`, `NULL_BRUSH`) di atas tombol *Maximize* untuk menjawab `HTMAXBUTTON`. | `src/main.rs`, `src/layout.rs` |
| **2** | **Mica Buram Tanpa `WS_EX_NOREDIRECTIONBITMAP` & Regresi Runtime 154** (`egoist/mygo#63`, `webview2feedback#5752`) | Window dibuat dengan `WS_EX_NOREDIRECTIONBITMAP` + `DwmExtendFrameIntoClientArea(-1)`; `DefaultBackgroundColor` (`A=0` Shell / `A=255` Content); fallback solid `#070B19`. | `src/backdrop.rs`, `ui/theme.css` |
| **3** | ***Frameless Maximize* Terpotong ~9px & Popup Multi-Monitor Melenceng** (`Re-Browser`, Microsoft Learn) | Tangani `WM_NCCALCSIZE` (clamp ke `MonitorInfo.rcWork`) saat *maximized*, dan panggil `NotifyParentWindowPositionChanged()` pada `WM_MOVE` / `WM_MOVING`. | `src/main.rs`, `src/layout.rs` |
| **4** | **Z-Order *Sibling* HWND & `DeferWindowPos` WebView2** (`webview2feedback#3085`, `#4787`) | Bungkus *non-overlapping* Layer 1 & Content Views dalam *Host Child `HWND`* (`BeginDeferWindowPos`); pisahkan seluruh overlay mengambang ke **Owned `WS_POPUP` `HWND`** (#39, #47). | `src/tab_manager.rs`, `src/layout.rs` |
| **5** | **Popup Login OAuth (`window.opener`), Profil Sumber & `isOAuthPopup`** (`egoist/mygo#702`, `FeatherBrowser#43`) | Pada `NewWindowRequested`, periksa `WindowFeatures` untuk membedakan OAuth Popup (`isOAuthPopup: true`, persistent owned window) vs tab reguler, pada **profil yang sama dengan controller sumber (`nyari-ws-{id}` atau `"nyari-private"`)**, lalu `put_NewWindow()` + `Complete()`. | `src/tab_manager.rs` |
| **6** | **`AddWebResourceRequestedFilter` *Deprecated*, UI Stall & YouTube Stream Pruning** (`wails#6530`, *Audit v3.3*) | Gunakan `ICoreWebView2_22::AddWebResourceRequestedFilterWithRequestSourceKinds` (`SOURCE_KINDS_ALL`) untuk `SCRIPT`, `SUBDOCUMENT`, `XHR`, `FETCH` (`Arc<Engine>`); YouTube pruner via skrip *main-world* `Response.prototype.json`/`JSON.parse` Proxy. | `src/shield.rs` |
| **7** | **Video Fullscreen Terjebak, Absennya *Download* & *Permission Handler*** | Implementasi `ContainsFullScreenElementChanged`, `DownloadStarting`, dan `PermissionRequested`. | `src/tab_manager.rs`, `src/downloads.rs` |
| **8** | **Crash `RPC_E_WRONG_THREAD` & *Deadlock* COM STA vs `tokio`** | Jalankan seluruh API COM WebView2 satu arah via `EventLoopProxy` ke Main Thread STA; larang `block_on` di UI thread. | `src/main.rs`, `src/memory_reclaimer.rs` |
| **9** | **Risiko OOM V8 Cap 512 MB & Flag Menimpa Default** | Batas V8 *heap* profil di Settings (**Eco 512MB / Balanced 1024MB / Unlimited**), gabungkan flag `--disable-features` secara aman + *LRU Active Cap*. | `src/environment.rs`, `src/memory_reclaimer.rs` |
| **10** | ***Race Condition* Pemulihan Crash & Salah PID *Renderer*** | Tunggu `BrowserProcessExited` sebelum buat ulang *environment*; cek `IsVisible == false` dan ambil PID via `GetProcessExtendedInfos()`. | `src/environment.rs`, `src/memory_reclaimer.rs` |
| **11** | **Batas 512 Byte `CredWriteW`, GCM Nonce Reuse, & Hidden-Form Phishing** (*Audit v3.3*) | Enkripsi JSON tunggal (`payload_cipher`) via `HKDF-SHA256` (salt 16B) + `AES-256-GCM` (nonce 12B, `origin_domain` AAD), 1 Master Key 32B (Base64) di OS, *user gesture* + suntik top-frame via one-shot CDP Isolated World. | `src/vault.rs` |
| **12** | **Keamanan Host Internal & Debugging Source Map** | Sajikan UI *release* dari memori (`include_dir!`) via custom scheme `nyari://` dengan *CSP nonce* `BCryptGenRandom` & `X-Frame-Options: DENY`. | `src/environment.rs`, `src/ipc.rs` |
| **13** | **Risiko Port Debugging TCP & CDP Buta *Cross-Origin Iframe*** | Nonaktifkan `--remote-debugging-port`; gunakan COM API `CallDevToolsProtocolMethodForSessionAsync` (`flatten: true`) di balik *Named Pipe* DACL. | `src/mcp_bridge.rs` |
| **14** | **Bug Startup +5 Detik Akibat Non-Default `ProfileName`** (`webview2feedback#5557`) | Shell UI di profil `default` (`ProfileName = nullptr`) untuk startup < 500 ms; profil Workspace (`nyari-ws-{id}`) diinisialisasi asinkron/lazy setelah jendela tampil. | `src/environment.rs`, `src/extensions.rs` |
| **15** | **Verifikasi OS WinRT & `CredUI` pada Win10/11 + Cross-Account Guard** (*Builder Check 3*) | Utama (Win10 1903+/Win11): `UserConsentVerifier::CheckAvailabilityAsync` + `IUserConsentVerifierInterop`; Fallback: `CredUIPromptForWindowsCredentialsW(CREDUIWIN_ENUMERATE_CURRENT_USER)` + **`LsaConnectUntrusted` / `LsaLogonUser` + `EqualSid(TokenUser)`** + `SecureZeroMemory`. | `src/vault.rs` |
| **16** | **Crash Multi-Instance `ERROR_INVALID_STATE` (`0x8007139F`) & Pipe Race** | Named Mutex (`Local\NyariBrowser_Instance`) + server Named Pipe DACL dibuat segera sebelum UI; instans kedua retry `WaitNamedPipeW` (maks 500 ms), kirim argumen, lalu keluar < 10 ms. | `src/main.rs` |
| **17** | **`K32EmptyWorkingSet` Shared PID, `BrowsingInstance`, & Kegagalan `TrySuspend`** (*Audit v3.3*) | Periksa `isSuccessful` dari `TrySuspend`; jika `false`, masuk `soft_sleep_degraded` (`MemoryUsageTargetLevel::LOW`); pisahkan `BrowsingInstance` via `nyari-tab://` (#50); `K32EmptyWorkingSet` hanya jika 100% tab pada PID tidur. | `src/memory_reclaimer.rs` |
| **18** | **Absennya `SiteOrigin`/`FrameOrigin` pada `WebResourceRequested`** | Resolver `source_url` berjenjang: (1) Header `Origin`/`Referer`, (2) Cache `NavigationStarting` per `NavigationId`, (3) Bypass `$third-party` jika kosong. | `src/shield.rs` |
| **19** | ***Double-Fire* `AcceleratorKeyPressed` & Hilangnya Fokus** | Filter ketat `KeyEventKind == KeyDown \| SystemKeyDown`, gunakan `put_IsBrowserAcceleratorKeyEnabled(FALSE)`, kirim `SHORTCUT_TRIGGERED`, & panggil `MoveFocus(PROGRAMMATIC)` saat Overlay Popup tutup. | `src/main.rs`, `src/tab_manager.rs` |
| **20** | **Resize Flicker `WS_EX_NOREDIRECTIONBITMAP` & CORS ES Modules `nyari://`** | Tangani `WM_ERASEBKGND` (kembalikan `1`), disinkronkan di `WM_WINDOWPOSCHANGED`, header `Access-Control-Allow-Origin: nyari://shell`, matikan *Service Worker*. | `src/main.rs`, `src/environment.rs` |
| **21** | **Kendala WebAuthn / Passkey Windows Hello di WebView2** | Perintah cepat `/open-in-edge` di `Ctrl+K` untuk membuka halaman aktif di Microsoft Edge sistem saat login Passkey dibutuhkan. | `src/omnibox.rs`, `ui/src/CommandPalette.tsx` |
| **22** | **Celah Fitur Harian (*Find-in-Page*, PiP Exit, External URI)** | `ICoreWebView2_28::Find` (fallback bawaan), keluar PiP via `document.exitPictureInPicture()`, dialog `LaunchingExternalUriScheme`. | `src/tab_manager.rs`, `src/downloads.rs` |
| **23** | **Runtime Hilang & *Orphan Process* saat Shutdown** | Dialog Win32 ramah jika runtime gagal; pada `WM_CLOSE` lakukan tulis atomik `session.json.tmp → rename`, sinyal checkpoint ke SQLite actor thread, dan `Close()` seluruh controller. | `src/main.rs`, `src/storage.rs` |
| **24** | **Situs Eksternal ke `nyari://` & Reverse IPC Tanpa `WebMessage`** (*Audit v3.3*) | Blokir navigasi `nyari://`, `nyari-glance://`, & `nyari-tab://` di seluruh *Content Controllers*; kunci `WebMessageEnabled` & `AreHostObjectsAllowed` = `FALSE`; Curtain/Vault/Reader pakai one-shot CDP Isolated World. | `src/tab_manager.rs`, `src/curtain.rs` |
| **25** | **Absennya `CustomDataPartitionId` Stabil & Isolasi Workspace** (*Audit v3.3*) | Gunakan 100% API stabil `webview2-com 0.39.1`: **Lazy Named Profile per Workspace (`nyari-ws-{id}`)** + `ICoreWebView2Profile8::Delete()` saat Workspace dihapus. | `src/environment.rs`, `src/tab_manager.rs` |
| **26** | **`K32EmptyWorkingSet` Ditolak (`ACCESS_DENIED`) di AppContainer** (*Audit R3*) | Perlakukan `K32EmptyWorkingSet` sebagai optimasi *best-effort* yang mengabaikan error `0x5` secara senyap; andalkan `TrySuspend()` dan Tier 2 Hard Unload. | `src/memory_reclaimer.rs` |
| **27** | **Resize Tepi Atas Mati & Win10 1px Frame Issue** (`tauri#16304`, `tao#1361`) | Pasang *paint-free resize border ring child `HWND`* (`SetWindowRgn` hole); khusus Win10 panggil `DwmExtendFrameIntoClientArea` dengan `cyTopHeight: 1`. | `src/main.rs`, `src/layout.rs` |
| **28** | **Popup OAuth Macet & Dialog Asinkron Terhenti** (*Audit R3*) | Daftarkan handler `WindowCloseRequested` (tutup controller OAuth saat `window.close()`), `ScreenCaptureStarting` (`getDisplayMedia`), `ScriptDialogOpening`, & `BasicAuthenticationRequested`. | `src/tab_manager.rs`, `src/downloads.rs` |
| **29** | **CSS Cosmetic & Curtain Diblokir *Trusted Types*** (*Audit R3*) | Larang `.innerHTML` pada skrip injeksi; gunakan `document.adoptedStyleSheets` (`CSSStyleSheet()`) atau CDP `Page.addStyleSheet`. | `src/shield.rs`, `src/curtain.rs` |
| **30** | **Urutan & Latensi `NewWindowRequested` Deferral** (*Builder Check 2*) | Pre-compile skrip injeksi di memori; setup STA sinkron **< 5 ms** dan penyelesaian callback async pembuatan controller + `put_NewWindow()` + `Complete()` **< 150 ms** (tanpa timeout artifisial). | `src/tab_manager.rs` |
| **31** | **Autofill Edge bawaan Bentrok dengan `vault.db`** (*Audit R3*) | Setel `put_IsGeneralAutofillEnabled(FALSE)` dan `put_IsPasswordAutosaveEnabled(FALSE)` pada setiap profil browsing + `EnableTrackingPrevention = TRUE`. | `src/environment.rs`, `src/vault.rs` |
| **32** | **Update Runtime *Side-by-Side* Berjalan Lama** (*Audit R3*) | Daftarkan `add_NewBrowserVersionAvailable` pada environment untuk menampilkan indikator update non-intrusif di *Sidebar*. | `src/environment.rs`, `ui/src/Sidebar.tsx` |
| **33** | **Per-Monitor DPI V2 Scaling Meleset (`125%–200%` / Multi-Monitor)** (*Audit v3.3*) | Aktifkan `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`; seluruh konstanta layout dalam DIP dikalikan `GetDpiForWindow(hwnd) / 96.0`; tangani `WM_DPICHANGED` & `put_RasterizationScale`. | `src/main.rs`, `src/layout.rs` |
| **34** | **Custom Scheme `nyari://` Terbaca Sebagai `Origin: null` / Non-Secure** (*Audit v3.3*) | Setel `put_TreatAsSecure(TRUE)` dan `put_HasAuthorityComponent(TRUE)` pada `ICoreWebView2CustomSchemeRegistration` agar ES Modules dan CORS `nyari://shell` bekerja sah. | `src/environment.rs` |
| **35** | **Alokasi `IStream`/`HGLOBAL` Berlebih pada Blokir Iklan `Shield`** (*Audit v3.3*) | Cache **satu objek `ICoreWebView2WebResourceResponse` statis (`403 Forbidden`, `Content = None`)** di memori STA untuk dipakai ulang di semua blokir iklan (*zero-allocation*). | `src/shield.rs` |
| **36** | **`SQLITE_BUSY` / Deadlock WAL Checkpoint saat `WM_CLOSE`** (*Audit v3.3*) | Kelola seluruh `rusqlite::Connection` pada **satu SQLite Actor Thread khusus**; `WM_CLOSE` mengirim `DbCommand::ShutdownAndCheckpoint` dengan timeout 150 ms. | `src/storage.rs`, `src/main.rs` |
| **37** | **Lubang `SetWindowRgn` Ring Tidak Berubah saat Resize/Maximize** (*Audit v3.3*) | Pada `WM_WINDOWPOSCHANGED`, sembunyikan Resize Border Ring (`SW_HIDE`) saat `Maximized`/`Fullscreen`, dan hitung ulang `CombineRgn(RGN_DIFF)` + `SetWindowRgn` saat `Restored`. | `src/layout.rs` |
| **38** | **Tier 2 Hard Unload Bangunkan Renderer & Hapus Draft Form** (*Audit v3.3*) | Ambil `scrollX`, `scrollY`, dan `hasUnsavedInput` **sebelum** Tier 1 Soft Sleep; tab dengan `hasUnsavedInput == true` dikecualikan dari Tier 2 Hard Unload. | `src/memory_reclaimer.rs` |
| **39** | **Sibling `WS_CHILD` HWND Gagal Alpha-Blend Overlay / Rounded Corners** (*Builder Check*) | Layer 1 & Content tetap `WS_CHILD` non-tumpang-tindih; seluruh overlay mengambang memakai **Owned Top-Level `WS_POPUP` `HWND`** (`DWMWCP_ROUND`, `DWMSBT_TRANSIENTWINDOW`) + `SET_OVERLAY_BOUNDS`. | `src/layout.rs`, `src/main.rs` |
| **40** | **Passive CDP `Runtime.enable` Memicu Blokir Anti-Bot Cloudflare/WAF** (*Builder Check*) | **Dilarang** memasang `Runtime.enable` pasif; Glance (`Alt+Klik`) memakai `nyari-glance://` (`http`/`https` validated); Curtain/Vault/Reader memakai **on-demand one-shot CDP** lalu segera `Runtime.disable`. | `src/tab_manager.rs`, `src/curtain.rs` |
| **41** | **`nyari-mcp` Kurang Stdio Transport, Tool Surface, & Target Guard** (*Builder Check*) | Mode `nyari.exe --mcp-stdio` (proxy ke `\\.\pipe\NyariBrowser_MCP_<SID>`, `\n`-JSON-RPC), 7 tool terkurasi, blocklist (`nyari://`, Private Tabs, OAuth), & approval per aksi/origin sesi. | `src/mcp_bridge.rs`, `src/main.rs` |
| **42** | **Secret Leak IPC Vault, Domain/Iframe AAD Ambigu, & Akun Tanpa Sandi** (*Builder Check*) | Buffering Capture di Rust via `promptId` (`Zeroizing<String>`, 60s TTL); lookup/AAD **exact Web Origin (`scheme://host[:port]`)** + injeksi **top-frame only**; `UNIQUE(origin_domain, username_hint)`; fail-closed pada akun tanpa PIN/sandi. | `src/vault.rs`, `src/ipc.rs` |
| **43** | **CRX3 `_metadata` E_ACCESSDENIED, `"key"` ID & Cakupan Ekstensi** (*Builder Check 3*) | Ekstraksi CRX3 `CrxFileHeader` (strip `_metadata/`, suntik `"key"` ke `manifest.json` agar ID kanonik `a`–`p` cocok, pakai `extension.Id()`), popup Owned `WS_POPUP` di **profil Workspace aktif** (`200x150`–`800x600 DIP`); nonaktif di `"nyari-private"`. | `src/extensions.rs`, `src/layout.rs` |
| **44** | **`shield.bin` vs Budget `< 15 MB` & `0` Unsolicited Network Requests** (*Builder Check*) | Embed seed filter terkompresi (~1.2 MB EasyList+EasyPrivacy+AdServers) di binary untuk kompilasi `shield.bin` offline; update remote hanya manual (`UPDATE_SHIELD_LISTS`) atau opt-in (`shield_auto_update`). | `src/shield.rs` |
| **45** | **Ambiguitas Isolasi Private Tab vs Penghapusan Workspace** (*Builder Check*) | Pisahkan Private Tabs ke profil khusus `"nyari-private"` dengan `put_IsInPrivateModeEnabled(TRUE)`, terlepas dari `ICoreWebView2Profile8::Delete()` Workspace dan bebas ekstensi. | `src/environment.rs`, `src/tab_manager.rs` |
| **46** | **Reading Mode Trusted Types & Celah IPC Bookmark/Settings/Sync** (*Builder Check*) | Reading Mode ekstrak JSON via one-shot CDP → render Preact VNode di `nyari://shell/`; lengkapi IPC Bookmarks CRUD, Settings/Clear-Data, Curtain list, Shield update, & Rust→Shell sync (`GLANCE_STATE_CHANGED`, `SPLIT_VIEW_CHANGED`). | `src/ipc.rs`, `ui/src/` |
| **47** | **Geometri `Layer 1` Vertikal, Priority Stack `Layer 2` & `WM_ACTIVATE`** (*Builder Check 3*) | `Layer 1` berbentuk-L (`32 DIP` top caption + sidebar via `SetWindowRgn`); Priority Stack tunggal di `Layer 2` (`Modal FIFO -> Palette/FindInPage -> Reading -> HoverSidebar`); Nyari Window-Family `WM_ACTIVATE` guard (`GetAncestor`); `zen_mode` + `SHORTCUT_TRIGGERED`. | `src/layout.rs`, `src/main.rs` |
| **48** | **Registry `extensions.json`, Origin Lock Popup & `chrome.tabs` Shim** (*Builder Check 3*) | Persistensi `extensions.json`; aktifkan `IsWebMessageEnabled(TRUE)` + lock `NavigationStarting`/`Source()` hanya pada dokumen `chrome-extension://{id}/` (`popup.html`/`options_page`, bukan Service Worker `#3852`); suntik `window.__NYARI_ACTIVE_TAB__` & handle `EXT_CREATE_TAB` + `put_Handled(TRUE)`. | `src/extensions.rs`, `src/tab_manager.rs` |
| **49** | **Kontradiksi Capture Vault Otomatis & Validasi Auth Win10** (*Builder Check 2*) | Hapus capture otomatis; wajibkan Pre-Submit `VAULT_CAPTURE_FORM` atau `VAULT_SAVE_MANUAL`; `NavigationCompleted` memancarkan `VAULT_AUTOFILL_AVAILABLE`; Win10 fallback `CredUI` + `LsaLogonUser` + `EqualSid` + `STATUS_ACCOUNT_RESTRICTION` fail-closed. | `src/vault.rs`, `src/ipc.rs` |
| **50** | **Diskriminator `BrowsingInstance` Middle-Click & MCP Eval Field Sandi** (*Builder Check 2*) | Interceptor `nyari-tab://` pada `auxclick`/`Ctrl+Klik` untuk memutus `BrowsingInstance`; tolak keras `browser_eval_isolated` bila ada `<input type="password">` di halaman/frame + redaksi `[REDACTED_PASSWORD]` di `browser_get_markdown_or_dom`. | `src/tab_manager.rs`, `src/mcp_bridge.rs` |

---

## 7. Peta Jalan Implementasi (*Phased Roadmap*)

* **Fase 1 — Fondasi Win32 STA, DPI V2, Owned Popup Overlays & SQLite Actor:**
  Setup proyek Rust (`tao`, `windows`, `webview2-com = "0.39"`, `zeroize`), manifest `PerMonitorV2` + `WM_DPICHANGED`, flag `--mcp-stdio`, Single-Instance Mutex + immediate DACL Named Pipe (`WaitNamedPipeW` retry), *Non-Overlapping Child `HWND`s* + *Owned `WS_POPUP` Overlay `HWND`s* (Priority Stack & Window-Family `WM_ACTIVATE` guard), *Dynamic Resize Border Ring `HWND`*, `WS_EX_NOREDIRECTIONBITMAP`, `WM_NCCALCSIZE`, `WM_ERASEBKGND`, `WM_WINDOWPOSCHANGED`, *Snap Overlay* (`HTMAXBUTTON`), `NotifyParentWindowPositionChanged`, `EventLoopProxy`, Dedicated SQLite Actor Thread, `CoreWebView2Environment13` (`ExclusiveUserDataFolderAccess`, `IsCustomCrashReportingEnabled`, `AreBrowserExtensionsEnabled`, skema `nyari://` dengan `TreatAsSecure` + `HasAuthorityComponent`, `add_NewBrowserVersionAvailable`, `BrowserProcessExited`), *graceful shutdown*, serta *Shell UI* Preact + Signals (`include_dir!`).
* **Fase 2 — *3-Tier Memory Lifecycle*, Named Profiles Asinkron, OAuth & Layout Adaptif:**
  Implementasi penyimpanan atomik `session.json` (tanpa Private Tabs), Lazy Named Profile per Workspace (`nyari-ws-{id}` + `ICoreWebView2Profile8::Delete()`), Private Tab (`"nyari-private"` + `IsInPrivateModeEnabled`), *true lazy-loading*, `nyari-tab://` *BrowsingInstance* isolation, *LRU Active Renderer Cap*, pre-sleep one-shot CDP snapshot (`scrollX/Y` + `hasUnsavedInput`), penjadwal *Tier 1 Soft Sleep* (`TrySuspend` + `isSuccessful` check + `MemoryUsageTargetLevel::LOW` fallback + *Shared-PID Guard* → `K32EmptyWorkingSet`), *Tier 2 Hard Unload* (guard `hasUnsavedInput`), `NewWindowRequested` (source-profile inheritance, < 5 ms STA setup / < 150 ms async deferral) + `WindowCloseRequested`, `NavigationStarting` `nyari://`, `nyari-glance://`, & `nyari-tab://` guard, `AcceleratorKeyPressed` (`SHORTCUT_TRIGGERED`), `ContainsFullScreenElementChanged`, PiP exit, serta toggle layout.
* **Fase 3 — Produktivitas, SQLite FTS5, On-Demand CDP, Reading Mode & Privasi:**
  Integrasi `history.db` (WAL + FTS5 upsert + query escaping + Bookmarks CRUD) untuk *omnibox* < 1 ms + `/open-in-edge`, *Split View* (`BeginDeferWindowPos` + sync event), *Glance* (Owned `WS_POPUP` via `nyari-glance://` tanpa `Runtime.enable`), *Curtain* (on-demand CDP Isolated World + `Runtime.disable` saat selesai), *Reading Mode* (one-shot CDP Readability JSON -> Preact VNode), *Find-in-Page* (`ICoreWebView2_28::Find`), `DownloadStarting`, `PermissionRequested`, `ScreenCaptureStarting`, `ScriptDialogOpening`, `BasicAuthenticationRequested`, `LaunchingExternalUriScheme`, `Shield` (embedded ~1.2 MB compressed seed + static cached 403 + YouTube *main-world* pruner).
* **Fase 4 — Vault HKDF+AES-GCM (`promptId`), Ekstensi CRX3 & `nyari.exe --mcp-stdio`:**
  Implementasi `vault.db` (`HKDF-SHA256` + `AES-256-GCM` dengan exact Web Origin AAD + `UNIQUE(origin_domain, username_hint)` + 32B Master Key Base64 di OS + pre-submit `VAULT_CAPTURE_FORM` `Zeroizing` `promptId` + `NavigationCompleted` `VAULT_AUTOFILL_AVAILABLE` + `UserConsentVerifier` & `CredUI` **`LsaLogonUser` + `EqualSid`** + top-frame one-shot CDP autofill), penonaktifan autofill Edge bawaan, pemuat ekstensi CRX3/unpacked (strip `_metadata/`, `"key"` injection, `extensions.json`) + *Owned `WS_POPUP` action popup* dengan `chrome.tabs` origin-locked shim (`EXT_CREATE_TAB` + `put_Handled(TRUE)`), serta MCP `\\.\pipe\NyariBrowser_MCP_<SID>` + `--mcp-stdio` dengan 7 tool terkurasi dan hard password-page lockout.
