# Nyari Browser — IPC Contracts & Storage Schema Specification

**Versi Spec:** 3.3 (50 Technical Mitigations — Builder-Check Verified)  
**Format Komunikasi:**
1. **Shell UI (`Layer 1` L-Shaped Base Chrome & `Layer 2` Owned Popup Overlay, profil `default`, origin `nyari://shell/`):**
   * **Backend → Frontend:** `PostWebMessageAsJson` dipanggil dari COM STA UI thread (dipicu oleh `tao::event_loop::EventLoopProxy`).
   * **Frontend → Backend:** `window.chrome.webview.postMessage(JSON.stringify(action))` (diaktifkan eksklusif pada `nyari://shell/`, serta secara origin-locked pada dokumen `chrome-extension://{id}/` untuk shim `chrome.tabs` dengan `NavigationStarting` lock & validasi `Source()`).
2. **Content Tabs (profil `nyari-ws-{id}` & `"nyari-private"`):**
   * `put_IsWebMessageEnabled(FALSE)` dan `put_AreHostObjectsAllowed(FALSE)` terkunci mutlak, dengan **nol** `Runtime.enable` pasif.
   * **Link Triggers (`Alt+Klik` & Middle/`Ctrl+Klik`):** Skrip `AddScriptToExecuteOnDocumentCreated` mengarahkan ke `nyari-glance://open?url=...` atau `nyari-tab://open?url=...` (dibatalkan di `NavigationStarting`, divalidasi `http`/`https`).
   * **On-Demand CDP Only:** Curtain picker (`Runtime.enable` sementara -> `Page.createIsolatedWorld("nyari_curtain")` -> `Runtime.addBinding` -> `delete globalThis.__nyari_bridge` + `Runtime.removeBinding` + `Runtime.disable`), pre-submit Vault capture / top-frame autofill, Reading Mode extraction, pre-sleep snapshot, dan MCP tool execution.

---

## 1. IPC Messages (Shell UI Preact ↔ Backend Rust)

### 1.1 Action Messages (Frontend Shell → Backend Rust)
Dikirim saat pengguna melakukan interaksi UI di *Layer 1* atau *Layer 2*.

```typescript
type UIAction =
  // Tab Lifecycle
  | { type: "CREATE_TAB"; url?: string; workspaceId: string; isPrivate?: boolean }
  | { type: "CLOSE_TAB"; tabId: string }
  | { type: "SWITCH_TAB"; tabId: string }
  | { type: "PIN_TAB"; tabId: string; isPinned: boolean }
  | { type: "TOGGLE_KEEP_AWAKE"; tabId: string }
  | { type: "DUPLICATE_TAB"; tabId: string }

  // Navigation
  | { type: "NAVIGATE"; tabId: string; urlOrQuery: string }
  | { type: "GO_BACK"; tabId: string }
  | { type: "GO_FORWARD"; tabId: string }
  | { type: "RELOAD"; tabId: string; ignoreCache?: boolean }
  | { type: "STOP_LOAD"; tabId: string }

  // Workspaces (Lazy Named Profiles)
  | { type: "CREATE_WORKSPACE"; name: string; icon: string }
  | { type: "UPDATE_WORKSPACE"; workspaceId: string; name?: string; icon?: string }
  | { type: "SWITCH_WORKSPACE"; workspaceId: string }
  | { type: "DELETE_WORKSPACE"; workspaceId: string }

  // Layout, Owned Popup Bounds (Priority Stack), Glance, Split View & Reading Mode
  | { type: "SET_LAYOUT"; mode: "vertical-expanded" | "vertical-collapsed" | "topbar" }
  | { type: "TOGGLE_ZEN" }
  | {
      type: "SET_OVERLAY_BOUNDS";
      visible: boolean;
      surface: "modal" | "command_palette" | "reading_mode" | "hover_sidebar" | null;
      xDip: number;
      yDip: number;
      widthDip: number;
      heightDip: number;
    }
  | { type: "SPLIT_VIEW"; leftTabId: string; rightTabId: string }
  | { type: "CLOSE_SPLIT" }
  | { type: "PROMOTE_GLANCE" }
  | { type: "CLOSE_GLANCE" }
  | { type: "TOGGLE_READING_MODE"; tabId: string }
  | { type: "TOGGLE_PIP"; tabId: string }
  | { type: "WINDOW_MINIMIZE" }
  | { type: "WINDOW_MAXIMIZE_RESTORE" }
  | { type: "WINDOW_CLOSE" }

  // Command Palette & Omnibox
  | { type: "QUERY_OMNIBOX"; query: string }
  | { type: "EXECUTE_COMMAND"; command: string; args?: string[] }
  | { type: "OPEN_IN_EDGE"; url: string }
  | { type: "CLOSE_OVERLAY" }

  // Bookmarks CRUD
  | { type: "CREATE_BOOKMARK"; title: string; url: string; parentFolder?: string }
  | { type: "DELETE_BOOKMARK"; bookmarkId: string }
  | { type: "LIST_BOOKMARKS" }

  // Settings & Browsing Data
  | { type: "GET_SETTINGS" }
  | { type: "UPDATE_SETTINGS"; patch: Partial<AppSettings> }
  | { type: "CLEAR_BROWSING_DATA"; workspaceId?: string; scopes: ("cache" | "cookies" | "history")[] }

  // Find-in-Page
  | { type: "FIND_IN_PAGE"; tabId: string; query: string; matchCase: boolean; wholeWord: boolean }
  | { type: "FIND_NEXT"; tabId: string; forward: boolean }
  | { type: "FIND_CLOSE"; tabId: string }

  // Downloads
  | { type: "PAUSE_DOWNLOAD"; downloadId: string }
  | { type: "RESUME_DOWNLOAD"; downloadId: string }
  | { type: "CANCEL_DOWNLOAD"; downloadId: string }
  | { type: "OPEN_DOWNLOAD_FOLDER"; downloadId: string }

  // Permissions, Script Dialogs, Basic Auth & External URI
  | { type: "RESOLVE_PERMISSION"; requestId: string; decision: "allow" | "deny" }
  | { type: "RESOLVE_SCRIPT_DIALOG"; requestId: string; accepted: boolean; promptText?: string }
  | { type: "RESOLVE_BASIC_AUTH"; requestId: string; cancelled: boolean; username?: string; password?: string }
  | { type: "RESOLVE_EXTERNAL_URI"; requestId: string; allowed: boolean }

  // Shield & Curtain
  | { type: "TOGGLE_SHIELD"; tabId: string; enabled: boolean }
  | { type: "UPDATE_SHIELD_LISTS" }
  | { type: "START_CURTAIN_PICKER"; tabId: string }
  | { type: "STOP_CURTAIN_PICKER"; tabId: string }
  | { type: "CURTAIN_HIDE_ELEMENT"; domain: string; selector: string }
  | { type: "CURTAIN_REMOVE_RULE"; domain: string; selector: string }
  | { type: "LIST_CURTAIN_RULES"; domain?: string }

  // Encrypted Vault (Pre-Submit Capture via promptId + Trusted Manual CRUD)
  | { type: "VAULT_CAPTURE_FORM"; tabId: string }
  | { type: "VAULT_CONFIRM_SAVE"; promptId: string; username: string }
  | { type: "VAULT_UNLOCK_AND_FILL"; tabId: string; credentialId: string }
  | { type: "VAULT_SAVE_MANUAL"; originDomain: string; username: string; password: string }
  | { type: "VAULT_LIST_CREDENTIALS" }
  | { type: "VAULT_DELETE_CREDENTIAL"; credentialId: string }
  | { type: "VAULT_DISMISS_PROMPT"; promptId: string }

  // Extensions (ICoreWebView2Profile7 + extensions.json + Owned Popup HWND)
  | { type: "LOAD_UNPACKED_EXTENSION"; workspaceId: string; folderPath: string }
  | { type: "INSTALL_CRX_EXTENSION"; workspaceId: string; crxPath: string }
  | { type: "TOGGLE_EXTENSION_ENABLED"; workspaceId: string; extensionId: string; enabled: boolean }
  | { type: "REMOVE_EXTENSION"; workspaceId: string; extensionId: string }
  | { type: "OPEN_EXTENSION_POPUP"; workspaceId: string; extensionId: string; anchorRectDip: RectDip }
  | { type: "OPEN_EXTENSION_OPTIONS"; workspaceId: string; extensionId: string }

  // MCP Agent Control (Named Pipe & --mcp-stdio)
  | { type: "SET_MCP_ENABLED"; enabled: boolean }
  | { type: "RESOLVE_MCP_ACTION"; requestId: string; decision: "allow_once" | "allow_origin_session" | "deny" }
  | { type: "STOP_MCP_SESSION" };
```

### 1.2 State Update Events (Backend Rust → Frontend Shell)
Dikirim oleh Rust melalui `PostWebMessageAsJson` untuk memperbarui Preact Signals secara reaktif.

```typescript
type UIEvent =
  | { type: "TAB_CREATED"; tab: TabMetadata }
  | { type: "TAB_UPDATED"; tabId: string; changes: Partial<TabMetadata> }
  | { type: "TAB_CLOSED"; tabId: string }
  | { type: "WORKSPACE_LIST_UPDATED"; activeWorkspaceId: string; workspaces: WorkspaceMetadata[] }
  | { type: "MEMORY_STATE_CHANGED"; tabId: string; state: "active" | "soft_sleep" | "soft_sleep_degraded" | "hard_unload"; ramBytes: number }
  | { type: "SHORTCUT_TRIGGERED"; action: "open_command_palette" | "open_find_in_page" | "close_active_overlay" }
  | { type: "CURTAIN_PICKER_STATE_CHANGED"; tabId: string; active: boolean }
  | { type: "OMNIBOX_RESULTS"; results: OmniboxResultItem[] }
  | { type: "BOOKMARKS_UPDATED"; bookmarks: BookmarkItem[] }
  | { type: "SETTINGS_UPDATED"; settings: AppSettings }
  | { type: "GLANCE_STATE_CHANGED"; open: boolean; url?: string; title?: string }
  | { type: "SPLIT_VIEW_CHANGED"; leftTabId: string | null; rightTabId: string | null }
  | { type: "READING_MODE_UPDATED"; tabId: string; active: boolean; article?: ReadingArticle }
  | { type: "CURTAIN_RULES_UPDATED"; rules: Record<string, string[]> }
  | { type: "SHIELD_STATUS_UPDATED"; updating: boolean; lastUpdated?: number; error?: string }
  | { type: "FIND_RESULT_UPDATED"; tabId: string; activeMatch: number; matchCount: number }
  | { type: "DOWNLOAD_PROGRESS"; item: DownloadItem }
  | { type: "PERMISSION_REQUESTED"; req: PermissionRequest }
  | { type: "SCRIPT_DIALOG_OPENING"; req: ScriptDialogRequest }
  | { type: "BASIC_AUTH_REQUESTED"; req: BasicAuthRequest }
  | { type: "EXTERNAL_URI_REQUESTED"; req: ExternalUriRequest }
  | { type: "VAULT_AUTOFILL_AVAILABLE"; tabId: string; originDomain: string; matches: { credentialId: string; usernameHint: string }[] }
  | { type: "VAULT_PROMPT_SAVE"; promptId: string; tabId: string; originDomain: string; username: string }
  | { type: "VAULT_CREDENTIALS_LISTED"; items: { credentialId: string; originDomain: string; usernameHint: string; updatedAt: number }[] }
  | { type: "VAULT_AUTH_RESULT"; success: boolean; error?: string }
  | { type: "EXTENSIONS_UPDATED"; workspaceId: string; extensions: ExtensionMetadata[] }
  | { type: "MCP_STATUS_CHANGED"; enabled: boolean; activeClient: boolean }
  | { type: "MCP_ACTION_REQUESTED"; req: McpActionRequest }
  | { type: "RUNTIME_UPDATE_AVAILABLE"; version: string }
  | { type: "BROWSER_PROCESS_CRASHED"; reason: string; recovering: boolean };

interface RectDip {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface AppSettings {
  version: "3.3";
  layout_mode: "vertical-expanded" | "vertical-collapsed" | "topbar";
  zen_mode: boolean;
  v8_heap_profile: "eco" | "balanced" | "unlimited";
  lru_active_cap: number;
  soft_sleep_timeout_sec: number;
  hard_unload_timeout_sec: number;
  search_engine_template: string;
  theme: "dark" | "light";
  shield_auto_update: boolean;
  mcp_enabled: boolean;
}

interface TabMetadata {
  id: string;
  workspaceId: string;
  title: string;
  url: string;
  faviconUrl: string;
  isPinned: boolean;
  isPrivate: boolean;
  isOAuthPopup: boolean;
  keepAwake: boolean;
  hasUnsavedInput: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  isLoading: boolean;
  isAudible: boolean;
  shieldBlockedCount: number;
  memoryState: "active" | "soft_sleep" | "soft_sleep_degraded" | "hard_unload";
  ramBytes: number;
}

interface WorkspaceMetadata {
  id: string;
  name: string;
  icon: string;
  profileName: string; // e.g., "nyari-ws-default"
}

interface BookmarkItem {
  id: string;
  title: string;
  url: string;
  parentFolder: string;
  createdAt: number;
}

interface ReadingArticle {
  title: string;
  byline?: string;
  blocks: { kind: "h1" | "h2" | "p" | "blockquote" | "pre" | "li"; text: string }[];
}

interface OmniboxResultItem {
  id: string;
  title: string;
  url: string;
  source: "tab" | "history" | "bookmark" | "command" | "search";
  icon?: string;
}

interface DownloadItem {
  id: string;
  fileName: string;
  receivedBytes: number;
  totalBytes: number;
  state: "in_progress" | "completed" | "cancelled" | "paused";
}

interface PermissionRequest {
  requestId: string;
  origin: string;
  kind: "camera" | "microphone" | "geolocation" | "notifications" | "clipboard" | "screen_capture";
}

interface ScriptDialogRequest {
  requestId: string;
  tabId: string;
  kind: "alert" | "confirm" | "prompt" | "beforeunload";
  message: string;
  defaultText?: string;
}

interface BasicAuthRequest {
  requestId: string;
  tabId: string;
  uri: string;
  realm: string;
}

interface ExternalUriRequest {
  requestId: string;
  tabId: string;
  uri: string;
}

interface ExtensionMetadata {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  hasPopup: boolean;
  hasOptionsPage: boolean;
  iconDataUrl?: string;
}

interface McpActionRequest {
  requestId: string;
  toolName: string;
  targetOrigin: string;
  summary: string;
}
```

### 1.3 Wire Protocol `nyari-mcp` (`\\.\pipe\NyariBrowser_MCP_<USER_SID>`)
* **Transport:** Named Pipe DACL-locked khusus SID pengguna; mode proxy `nyari.exe --mcp-stdio` menjembatani `stdin`/`stdout` ↔ Named Pipe.
* **Framing:** Newline-delimited (`\n`) UTF-8 JSON-RPC 2.0.
* **Curated Tools:** `browser_list_tabs`, `browser_navigate`, `browser_get_markdown_or_dom`, `browser_click`, `browser_type`, `browser_screenshot`, `browser_eval_isolated`. (Menolak `nyari://shell/`, Private Tabs, OAuth popups `isOAuthPopup: true`, menolak total `browser_eval_isolated` saat ada `<input type="password">`, serta menyensor nilai field sandi menjadi `"[REDACTED_PASSWORD]"` pada `browser_get_markdown_or_dom`.)

### 1.4 Extension Popup / Options Shim IPC (`chrome-extension://{id}/` → Rust)
* **Pre-Injected Sync Snapshot (`AddScriptToExecuteOnDocumentCreated`):**
  `window.__NYARI_ACTIVE_TAB__ = { id, index: 0, windowId: 1, active: true, url, title };` (memungkinkan `chrome.tabs.query({ active: true, currentWindow: true })` selesai secara sinkron tanpa RPC).
* **WebMessage (`chrome-extension://{id}/` Verified via `Source()`):**
  ```typescript
  type ExtensionShimMessage = { type: "EXT_CREATE_TAB"; url: string };
  ```
  (Controller ekstensi mengunci `NavigationStarting` agar tidak keluar dari `chrome-extension://{id}/` serta menyetel `put_Handled(TRUE)` pada `NewWindowRequested` ke tab Workspace aktif.)

---

## 2. Skema Database SQLite (`%LOCALAPPDATA%\NyariBrowser\`)

Seluruh koneksi SQLite (`history.db` dan `vault.db`) dimiliki eksklusif oleh **Dedicated SQLite Actor Thread** (Mitigasi #36).

### 2.1 Database Riwayat & Bookmark (`history.db`)
Menggunakan **SQLite WAL Mode** + **FTS5 Virtual Table** untuk pencarian < 1 ms yang terisolasi per Workspace (`workspace_id`).

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

-- Tabel Kunjungan Riwayat (Deduplikasi per Workspace + URL)
CREATE TABLE IF NOT EXISTS visits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL DEFAULT 'ws-default',
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    visit_time INTEGER NOT NULL, -- Unix timestamp (ms)
    visit_count INTEGER NOT NULL DEFAULT 1,
    favicon_blob BLOB,
    UNIQUE(workspace_id, url)
);

CREATE INDEX IF NOT EXISTS idx_visits_ws_time ON visits(workspace_id, visit_time DESC);

-- Virtual Table FTS5 untuk Autocomplete Super Cepat
CREATE VIRTUAL TABLE IF NOT EXISTS visits_fts USING fts5(
    title,
    url,
    content='visits',
    content_rowid='id'
);

-- Triggers untuk Sinkronisasi FTS5 Otomatis
CREATE TRIGGER IF NOT EXISTS visits_ai AFTER INSERT ON visits BEGIN
  INSERT INTO visits_fts(rowid, title, url) VALUES (new.id, new.title, new.url);
END;

CREATE TRIGGER IF NOT EXISTS visits_ad AFTER DELETE ON visits BEGIN
  INSERT INTO visits_fts(visits_fts, rowid, title, url) VALUES('delete', old.id, old.title, old.url);
END;

CREATE TRIGGER IF NOT EXISTS visits_au AFTER UPDATE OF title, url ON visits BEGIN
  INSERT INTO visits_fts(visits_fts, rowid, title, url) VALUES('delete', old.id, old.title, old.url);
  INSERT INTO visits_fts(rowid, title, url) VALUES (new.id, new.title, new.url);
END;

-- Tabel Bookmarks
CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    parent_folder TEXT DEFAULT 'root',
    created_at INTEGER NOT NULL
);
```

* **Query Upsert Kunjungan Wajib (Scoped per Workspace):**
  ```sql
  INSERT INTO visits (workspace_id, url, title, visit_time, visit_count, favicon_blob)
  VALUES (?1, ?2, ?3, ?4, 1, ?5)
  ON CONFLICT(workspace_id, url) DO UPDATE SET
      visit_count  = visits.visit_count + 1,
      visit_time   = excluded.visit_time,
      title        = CASE WHEN excluded.title != '' THEN excluded.title ELSE visits.title END,
      favicon_blob = COALESCE(excluded.favicon_blob, visits.favicon_blob);
  ```
  (`CLEAR_BROWSING_DATA { workspaceId, scopes: ["history"] }` menghapus baris `WHERE workspace_id = ?1` jika `workspaceId` diberikan, atau menghapus seluruh tabel jika `workspaceId` tidak diisi.)
* **Sanitasi Kueri FTS5 Omnibox:** Input mentah dipecah berdasarkan spasi, setiap token di-escape `"` → `""`, lalu dibungkus `"token"*` sebelum masuk `visits_fts MATCH ?1` agar karakter URL (`.`, `:`, `-`, `/`) tidak memicu error sintaks FTS5.

### 2.2 Database Vault Terenkripsi (`vault.db`)
Menggunakan **Master Key 32-byte (Base64 di Windows Credential Manager)** + **HKDF-SHA256 per baris** + **AES-256-GCM dengan exact Web Origin (`scheme://host[:port]`) sebagai AAD**. Username & password digabung dalam satu objek JSON terenkripsi (`payload_cipher`) untuk mencegah *AES-GCM nonce reuse*.

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    origin_domain TEXT NOT NULL,   -- Exact Web Origin e.g. "https://accounts.google.com" (digunakan sebagai AES-GCM AAD)
    username_hint TEXT NOT NULL,   -- Hint non-rahasia untuk daftar UI sebelum unlock OS
    payload_cipher BLOB NOT NULL,  -- AES-256-GCM ciphertext dari JSON {"username":"...","password":"..."}
    salt BLOB NOT NULL,            -- 16-byte CSPRNG HKDF-SHA256 salt unik per baris
    nonce BLOB NOT NULL,           -- 12-byte CSPRNG AES-GCM nonce unik per baris
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(origin_domain, username_hint)
);

CREATE INDEX IF NOT EXISTS idx_cred_domain ON credentials(origin_domain);
```

---

## 3. Spesifikasi File Konfigurasi JSON

### 3.1 `settings.json`
```json
{
  "version": "3.3",
  "layout_mode": "vertical-expanded",
  "zen_mode": false,
  "v8_heap_profile": "balanced",
  "lru_active_cap": 4,
  "soft_sleep_timeout_sec": 180,
  "hard_unload_timeout_sec": 1500,
  "search_engine_template": "https://duckduckgo.com/?q=%s",
  "theme": "dark",
  "shield_auto_update": false,
  "mcp_enabled": false
}
```

### 3.2 `session.json` (Ditulis Atomik via `.tmp`)
> **Invarian Privasi:** Tab dengan `is_private: true` **tidak pernah** ditulis ke `session.json`.

```json
{
  "active_workspace_id": "ws-default",
  "workspaces": [
    { "id": "ws-default", "name": "Utama", "icon": "home", "profile_name": "nyari-ws-default" },
    { "id": "ws-work", "name": "Kerja", "icon": "briefcase", "profile_name": "nyari-ws-work" }
  ],
  "tabs": [
    {
      "id": "tab-101",
      "workspace_id": "ws-default",
      "title": "GitHub",
      "url": "https://github.com",
      "is_pinned": true,
      "keep_awake": false,
      "scroll_x": 0,
      "scroll_y": 420
    }
  ],
  "split_view": null
}
```

### 3.3 `extensions.json` (Registry Metadata Ekstensi Terpersistensi)
```json
{
  "extensions": [
    {
      "id": "cjpalhdlnbpafiamejdnhcphjbkeiagm",
      "workspace_id": "ws-default",
      "name": "Example Extension",
      "version": "1.0.0",
      "enabled": true,
      "folder_path": "%LOCALAPPDATA%\\NyariBrowser\\Extensions\\cjpalhdlnbpafiamejdnhcphjbkeiagm\\1.0.0",
      "default_popup": "popup.html",
      "options_page": "options.html",
      "icon_data_url": "data:image/png;base64,..."
    }
  ]
}
```

---

## 4. Matriks Konstanta Layout (`layout.rs` ↔ `theme.css`)

Seluruh nilai di bawah ini adalah **Device-Independent Pixels (DIP / Logical Pixels @ 96 DPI)**. Pada sisi Win32 (`SetWindowPos`, `BeginDeferWindowPos`, `put_Bounds`, `SetWindowRgn`), `layout.rs` wajib mengalikan nilai DIP dengan `scale = GetDpiForWindow(hwnd) as f64 / 96.0`.

| Parameter Layout | Nilai Logis (DIP) | Catatan Kompatibilitas & DPI V2 |
| :--- | :--- | :--- |
| **Minimum Window Width** | `330 DIP` | Menjamin dukungan Snap Layouts 1/3 layar Win11 pada semua skala DPI |
| **Sidebar Expanded Width** | `230 DIP` | Tempat tab title + memory indicator + tombol ekstensi |
| **Sidebar Collapsed Width** | `48 DIP` | Rel favicon saja |
| **Vertical Top Caption Height** | `32 DIP` | Bar caption atas pada mode vertikal (`Layer 1` berbentuk-L via `SetWindowRgn`) untuk Window Controls & Snap Zone |
| **Top Bar Height** | `42 DIP` | Mode horizontal klasik |
| **Find-in-Page Overlay Box** | `340 DIP x 44 DIP` | Priority 2 di `Layer 2` (pojok kanan atas kanvas web, seragam untuk semua mode layout) |
| **Resize Border Hole Margin** | `6 DIP` | Ring child HWND (`SetWindowRgn` dihitung ulang saat resize; `SW_HIDE` saat maximized/fullscreen) |
| **Snap Maximize Zone** | `46 DIP x 32 DIP` | Area `HTMAXBUTTON` di pojok kanan atas Shell |
| **Extension Popup Clamping** | `200x150` – `800x600 DIP` | Batas ukuran minimum–maksimum Owned `WS_POPUP` `HWND` action ekstensi |
