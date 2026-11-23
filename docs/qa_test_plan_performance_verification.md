# Nyari Browser — Verification & Benchmark Test Plan

Dokumen ini memuat prosedur pengujian manual dan otomatis untuk memverifikasi bahwa **Nyari Browser v3.3** memenuhi target performa, efisiensi RAM, serta kebal terhadap **50 celah teknis**.

---

## 1. Metric Target Verification Checklist

| Metrik Performa | Target Nyari | Cara Mengukur / Tool | Status Pass/Fail |
| :--- | :--- | :--- | :--- |
| **Binary Size** | **< 15 MB** | Cek ukuran file `nyari.exe` di target release build (`cargo build --release`, termasuk embedded UI & seed `Shield`). | [ ] PASS |
| **Startup Time** | **< 500 ms** | Gunakan `Windows Performance Analyzer` (WPA) / Query Performance Counter dari Win32 `GetTickCount64` hingga `WM_PAINT` pertama. | [ ] PASS |
| **Cold Start RAM** | **150 - 210 MB** | Buka 1 tab (`https://example.com`), ukur *Private Working Set* total seluruh proses `nyari.exe` & WebView2 via Task Manager / Process Explorer. | [ ] PASS |
| **5 Active Tabs RAM** | **300 - 450 MB** | Buka 5 tab populer (Wikipedia, GitHub, Reddit, News, Docs). Ukur *Private Working Set*. | [ ] PASS |
| **15 Tabs (Soft Sleep)** | **450 - 650 MB** | Buka 15 tab, biarkan 11 tab idle > 3 menit hingga status berubah menjadi *Soft Sleep*. Ukur penurunannya. | [ ] PASS |
| **Omnibox Search Query** | **< 1 ms** | Jalankan FTS5 query pada `history.db` dengan 10.000 baris riwayat (termasuk karakter `.`, `:`, `-`). Cek durasi di log Rust. | [ ] PASS |

---

## 2. Test Cases Mitigasi Audit Teknis

### TC-01: Windows 11 Snap Layouts (`HTMAXBUTTON`)
* **Langkah:** Arahkan kursor (*hover*) ke tombol Maximize di sudut kanan atas jendela Nyari.
* **Hasil Diharapkan:** Menu *Snap Layouts* Windows 11 wajib muncul seketika tanpa terhalang atau tertahan oleh kanvas WebView2.

### TC-02: Frameless Resize & Win10 Border Test
* **Langkah:** Arahkan kursor ke 4 tepi jendela (termasuk tepi atas) dan lakukan drag-resize. Uji pada Windows 10 dan Windows 11 dalam mode *normal* dan *maximized*.
* **Hasil Diharapkan:** Drag-resize berjalan mulus di semua tepi. Saat *maximized*, jendela tidak terpotong 9px di luar monitor dan tidak ada garis putih di Win10.

### TC-03: Single Instance Fast Handover & Cold-Start Pipe Race
* **Langkah:** Jalankan `nyari.exe`. Saat jendela utama terbuka, buka terminal dan jalankan `nyari.exe https://github.com`. Uji juga meluncurkan 2 proses `nyari.exe` secara bersamaan via script PowerShell.
* **Hasil Diharapkan:** Instans kedua wajib berhasil terhubung via `WaitNamedPipeW` dan keluar dalam **< 10 ms** tanpa error `ERROR_INVALID_STATE (0x8007139F)`. Instans pertama langsung membuka tab baru `https://github.com`.

### TC-04: OAuth Login, Popup/Tab Discriminator, Source-Profile Inheritance & Timing
* **Langkah:** Uji `window.open()` berdimensi OAuth popup (`is_popup == true`) dan `<a target="_blank">` biasa (`is_popup == false`) baik di tab Workspace (`nyari-ws-default`) maupun **Private Tab (`"nyari-private"`)**. Saat popup OAuth terbuka, klik jendela utama untuk menyalin kode 2FA.
* **Hasil Diharapkan:** Setup sinkron STA sebelum pembuatan controller **< 5 ms** dan penyelesaian `GetDeferral()` **< 150 ms**. Popup OAuth terbuka di Top-Level Owned Popup Window (`isOAuthPopup: true`) dan **tidak menutup** saat mengklik `main_hwnd` (2FA), sedangkan `<a target="_blank">` terbuka sebagai tab terkelola (`isOAuthPopup: false`). Keduanya mewarisi profil sumber (`nyari-ws-default` atau `"nyari-private"`), dan popup OAuth menutup otomatis saat `window.close()` tanpa memutus `window.opener`.

### TC-05: 3-Tier Memory Lifecycle, `TrySuspend` Degraded Fallback & AppContainer Guard
* **Langkah:**
  1. Buka 6 tab (termasuk 1 tab dengan transaksi IndexedDB / audio aktif agar `TrySuspend` mengembalikan `isSuccessful = false`).
  2. Pastikan LRU active cap memindahkan tab tertua ke *Soft Sleep* (dan tab dengan lock ke `soft_sleep_degraded`).
  3. Cek Task Manager: pastikan RAM fisik berkurang 60–80% untuk tab `soft_sleep`.
  4. Periksa log Rust: pastikan tidak ada `panic` akibat error `0x5 ACCESS_DENIED` saat `K32EmptyWorkingSet` mengeksekusi PID AppContainer.
  5. Klik kembali tab yang sleep: tab harus kembali aktif dalam **< 50 ms** tanpa me-*reload* halaman.

### TC-06: Vault HKDF+AES-GCM Pre-Submit Capture, Win10 `LsaLogonUser` + `EqualSid` & Exact Origin AAD
* **Langkah:**
  1. Isi form login `https://example.com` lalu klik `VAULT_CAPTURE_FORM` sebelum men-submit dan konfirmasi simpan (`VAULT_CONFIRM_SAVE`).
  2. Buka ulang browser, kunjungi `https://example.com` (`NavigationCompleted` → `VAULT_AUTOFILL_AVAILABLE`), lalu lakukan autofill pada Windows 11 (Windows Hello), Windows 10 tanpa Hello (`CredUI` + `LsaLogonUser` + `EqualSid`), uji coba memasukkan kredensial **akun lokal Windows lain**, form di dalam cross-origin `<iframe>`, serta akun passwordless (`STATUS_ACCOUNT_RESTRICTION`).
* **Hasil Diharapkan:** Auth OS sukses untuk user pemilik akun pada Win11 & Win10 `LsaLogonUser`, **menolak password akun lokal lain** (`EqualSid` check), dan fail-closed pada akun tanpa PIN/sandi (`STATUS_ACCOUNT_RESTRICTION`); cross-origin `<iframe>` ditolak; `vault.db` menyimpan `payload_cipher`, `salt` (16B), `nonce` (12B), `UNIQUE(origin_domain, username_hint)`, dan pemindahan ciphertext lintas origin gagal dekripsi.

### TC-07: Passkey Escape Hatch (`/open-in-edge`)
* **Langkah:** Buka situs yang membutuhkan Passkey WebAuthn. Ketik `Ctrl+K`, lalu jalankan `/open-in-edge`.
* **Hasil Diharapkan:** Microsoft Edge sistem terbuka secara mulus membawa URL halaman aktif yang sama.

### TC-08: Trusted Types Bypass, Embedded Seed & YouTube JSON Pruner
* **Langkah:** Jalankan Nyari untuk pertama kali dalam keadaan offline, lalu buka YouTube dan Gmail (situs *Trusted Types* ketat) dengan `Shield` dan `Curtain` aktif.
* **Hasil Diharapkan:** `shield.bin` terkompilasi lokal dari embedded seed dengan **0 request jaringan**, iklan jaringan diblokir menggunakan static cached 403 response, payload JSON YouTube terpotong di sisi klien, dan `Curtain`/`Reading Mode` berjalan tanpa error `This document requires 'TrustedHTML' assignment`.

### TC-09: Per-Monitor DPI V2 (`WM_DPICHANGED`), `Layer 1` L-Shaped Caption & Resize Ring Test
* **Langkah:** Jalankan Nyari di mode `vertical-expanded` dan `vertical-collapsed` pada dual-monitor (Monitor A 100% DPI, Monitor B 150% DPI). Seret jendela antar-monitor, resize jendela, uji `HTMAXBUTTON` *Snap Layouts*, maksimalkan (*Maximize*), lalu klik elemen web di area tengah layar.
* **Hasil Diharapkan:** Sidebar (`230`/`48 DIP`), `32 DIP` top caption strip (`Layer 1` L-shaped `SetWindowRgn`), dan kanvas WebView2 tetap non-tumpang-tindih dan proporsional; `Snap Overlay HWND` tetap berfungsi di pojok kanan atas pada mode vertikal; *Resize Border Ring HWND* tidak menghalangi klik mouse setelah resize atau saat maximized.

### TC-10: Workspace Named Profile Isolation, Private Profile `"nyari-private"` & Cleanup
* **Langkah:** Login ke akun A pada Workspace *Utama* (`nyari-ws-default`) dan akun B pada Workspace *Kerja* (`nyari-ws-work`). Buka Private Tab (`Ctrl+Shift+N`), login akun C, lalu hapus Workspace *Kerja* saat Private Tab masih terbuka. Terakhir tutup Private Tab.
* **Hasil Diharapkan:** Sesi akun A, B, dan C terisolasi 100%; ekstensi Workspace nonaktif di Private Tab; penghapusan Workspace *Kerja* menghapus folder `nyari-ws-work` via `ICoreWebView2Profile8::Delete()` tanpa mengganggu Private Tab (`"nyari-private"`); Private Tab tidak tersisa di `session.json` atau `history.db`.

### TC-11: On-Demand CDP Curtain Isolation (`nyari_curtain`) & Dirty-Form Unload Guard
* **Langkah:**
  1. Aktifkan mode *Curtain* (`Ctrl+Shift+H`), lalu **selagi picker aktif**, buka DevTools Console pada halaman web target dan periksa `window.__nyari_bridge` serta `window.chrome.webview`.
  2. Pilih 1 elemen, lalu tutup picker dan verifikasi pembersihan binding CDP.
  3. Ketik teks di `<textarea>` pada satu tab tanpa men-submit, lalu biarkan idle > 25 menit.
* **Hasil Diharapkan:** Di console *main world* halaman web, `window.__nyari_bridge` dan `window.chrome.webview` tetap `undefined` baik saat picker aktif maupun setelah ditutup (`executionContextName: "nyari_curtain"` + `delete globalThis.__nyari_bridge` + `Runtime.removeBinding` + `Runtime.disable`); tab berisi draft formulir (`hasUnsavedInput == true`) tetap berada di *Tier 1 Soft Sleep* dan tidak di-unload oleh *Tier 2*.

### TC-12: SQLite FTS5 Workspace Upsert, Scoped Clear History & Clean WAL Checkpoint
* **Langkah:** Kunjungi `https://github.com` 5 kali di `ws-default` dan kunjungi `https://gitlab.com` di `ws-work`; cari `"https://github.com"` atau `"c++"` di Omnibox (`Ctrl+K`). Jalankan `CLEAR_BROWSING_DATA { workspaceId: "ws-default", scopes: ["history"] }`, lalu tutup browser lewat `WM_CLOSE`.
* **Hasil Diharapkan:** Tabel `visits` hanya menyimpan 1 baris untuk `(ws-default, https://github.com)` dengan `visit_count = 5`; pencarian Omnibox tidak crash pada karakter khusus; pembersihan history `ws-default` tidak menghapus riwayat `ws-work`; dan setelah `WM_CLOSE`, `history.db-wal` & `vault.db-wal` ter-truncate menjadi 0 byte tanpa `SQLITE_BUSY`.

### TC-13: Owned `WS_POPUP` Priority Stack, `WM_ACTIVATE` Guard, Uniform `Find-in-Page` & `nyari-tab://`
* **Langkah:**
  1. Ciutkan sidebar (`48 DIP`), tekan `Ctrl+F` untuk *Find-in-Page*, arahkan kursor (*hover*) lalu klik item di Hover-Sidebar; buka `Ctrl+K` Command Palette (`Layer 2`), ketik di inputnya; picu dialog izin sementara palette terbuka; dan tekan `Alt+Tab` ke aplikasi lain.
  2. Buka tautan dengan klik tengah / `Ctrl+Klik` (`nyari-tab://`) dan lakukan `Alt+Klik` (`nyari-glance://`) di situs anti-bot (Cloudflare Turnstile / fingerprinting).
* **Hasil Diharapkan:** `Ctrl+F` tampil seragam di `Layer 2` meski sidebar diciutkan; klik di dalam `Layer 2` / Hover-Sidebar tidak menutup popup (Window-Family `WM_ACTIVATE` guard); Priority 1 dialog tampil tanpa bertabrakan dengan palette; klik tengah/`Ctrl+Klik` memutus `BrowsingInstance` via `nyari-tab://` sehingga tab baru sukses masuk `TrySuspend`; dan situs anti-bot tidak mendeteksi `Runtime.enable` pasif.

### TC-14: `nyari.exe --mcp-stdio` Lockout, Vault `promptId` & CRX3 (`_metadata` / `"key"` / `chrome.tabs` Shim)
* **Langkah:**
  1. Aktifkan MCP (`SET_MCP_ENABLED = true`), hubungkan via `nyari.exe --mcp-stdio`, coba akses `nyari://shell/`, Private Tab, OAuth popup (`isOAuthPopup: true`), jalankan `browser_eval_isolated` pada halaman login yang memiliki `<input type="password">`, serta jalankan `browser_get_markdown_or_dom` pada form login terisi.
  2. Jalankan `VAULT_CAPTURE_FORM` dan inspeksi WebMessage ke Shell UI.
  3. Instal paket `.crx` (CRX3) yang memiliki folder `_metadata/` dan tidak memiliki `"key"` di `manifest.json` dengan `popup.html` pemanggil `chrome.tabs.query` & `chrome.tabs.create`, restart Nyari, lalu buka popup, picu `chrome.tabs.create`, dan uji navigasi popup ke `https://example.com`.
* **Hasil Diharapkan:** MCP menolak `nyari://`, Private Tab, OAuth popup, dan menolak keras `browser_eval_isolated` pada halaman dengan `<input type="password">`, sementara `browser_get_markdown_or_dom` menyensor field sandi menjadi `"[REDACTED_PASSWORD]"`; `VAULT_PROMPT_SAVE` hanya mengirim `username` tanpa plaintext password; `.crx` terpasang sukses (`_metadata/` terhapus, `"key"` terinjeksi, ID `extension.Id()` cocok di `extensions.json`, bertahan setelah restart), `chrome.tabs.query`/`create` bekerja pada tab Workspace tanpa jendela WebView2 yatim (`#3841`), dan navigasi keluar `chrome-extension://{id}/` diblokir.
