use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Adaptive window layout modes supported by the Nyari Shell UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutMode {
    VerticalExpanded,
    VerticalCollapsed,
    Topbar,
}

/// Priority-stacked surfaces hosted inside the Layer 2 owned popup window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlaySurface {
    Modal,
    CommandPalette,
    ReadingMode,
    HoverSidebar,
}

/// 3-tier tab renderer lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryState {
    Active,
    SoftSleep,
    SoftSleepDegraded,
    HardUnload,
}

/// V8 old-space heap cap profiles applied to WebView2 environment flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V8HeapProfile {
    Eco,
    Balanced,
    Unlimited,
}

/// Shell UI color theme modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Data categories eligible for `CLEAR_BROWSING_DATA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearScope {
    Cache,
    Cookies,
    History,
}

/// User resolution choices for `PERMISSION_REQUESTED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// Approval scopes for `RESOLVE_MCP_ACTION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpApprovalDecision {
    AllowOnce,
    AllowOriginSession,
    Deny,
}

/// Global keyboard shortcuts routed from WebView2 `AcceleratorKeyPressed` to the Shell UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    OpenCommandPalette,
    OpenFindInPage,
    CloseActiveOverlay,
}

/// AST block kinds extracted for Reading Mode rendering in `nyari://shell/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingBlockKind {
    H1,
    H2,
    P,
    Blockquote,
    Pre,
    Li,
}

/// Provenance of an Omnibox / Command Palette suggestion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmniboxSource {
    Tab,
    History,
    Bookmark,
    Command,
    Search,
}

/// Lifecycle state of an active or completed download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    InProgress,
    Completed,
    Cancelled,
    Paused,
}

/// Web permission categories surfaced to the user in Layer 2 modals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    Clipboard,
    ScreenCapture,
}

/// JavaScript dialog variants intercepted by `ScriptDialogOpening`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptDialogKind {
    Alert,
    Confirm,
    Prompt,
    Beforeunload,
}

/// Logical rectangle in Device-Independent Pixels (96 DPI baseline).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectDip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Persisted browser configuration (`settings.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub version: String,
    pub layout_mode: LayoutMode,
    pub zen_mode: bool,
    pub v8_heap_profile: V8HeapProfile,
    pub lru_active_cap: usize,
    pub soft_sleep_timeout_sec: u64,
    pub hard_unload_timeout_sec: u64,
    pub search_engine_template: String,
    pub theme: ThemeMode,
    pub shield_auto_update: bool,
    pub mcp_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: "3.3".to_string(),
            layout_mode: LayoutMode::VerticalExpanded,
            zen_mode: false,
            v8_heap_profile: V8HeapProfile::Balanced,
            lru_active_cap: 4,
            soft_sleep_timeout_sec: 180,
            hard_unload_timeout_sec: 1500,
            search_engine_template: "https://duckduckgo.com/?q=%s".to_string(),
            theme: ThemeMode::Dark,
            shield_auto_update: false,
            mcp_enabled: false,
        }
    }
}

impl AppSettings {
    /// Applies a partial settings update from `UIAction::UpdateSettings` in place.
    pub fn apply_patch(&mut self, patch: AppSettingsPatch) {
        if let Some(version) = patch.version {
            self.version = version;
        }
        if let Some(layout_mode) = patch.layout_mode {
            self.layout_mode = layout_mode;
        }
        if let Some(zen_mode) = patch.zen_mode {
            self.zen_mode = zen_mode;
        }
        if let Some(v8_heap_profile) = patch.v8_heap_profile {
            self.v8_heap_profile = v8_heap_profile;
        }
        if let Some(lru_active_cap) = patch.lru_active_cap {
            self.lru_active_cap = lru_active_cap;
        }
        if let Some(soft_sleep_timeout_sec) = patch.soft_sleep_timeout_sec {
            self.soft_sleep_timeout_sec = soft_sleep_timeout_sec;
        }
        if let Some(hard_unload_timeout_sec) = patch.hard_unload_timeout_sec {
            self.hard_unload_timeout_sec = hard_unload_timeout_sec;
        }
        if let Some(search_engine_template) = patch.search_engine_template {
            self.search_engine_template = search_engine_template;
        }
        if let Some(theme) = patch.theme {
            self.theme = theme;
        }
        if let Some(shield_auto_update) = patch.shield_auto_update {
            self.shield_auto_update = shield_auto_update;
        }
        if let Some(mcp_enabled) = patch.mcp_enabled {
            self.mcp_enabled = mcp_enabled;
        }
    }
}

/// Partial update payload mirroring TypeScript `Partial<AppSettings>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettingsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_mode: Option<LayoutMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zen_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v8_heap_profile: Option<V8HeapProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lru_active_cap: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_sleep_timeout_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_unload_timeout_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_engine_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shield_auto_update: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_enabled: Option<bool>,
}

/// Full tab state snapshot pushed to the Shell UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabMetadata {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub url: String,
    pub favicon_url: String,
    pub is_pinned: bool,
    pub is_private: bool,
    // Keep exact "isOAuthPopup" capitalization rather than serde's "isOauthPopup".
    #[serde(rename = "isOAuthPopup")]
    pub is_oauth_popup: bool,
    pub keep_awake: bool,
    pub has_unsaved_input: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub is_loading: bool,
    pub is_audible: bool,
    pub shield_blocked_count: u64,
    pub memory_state: MemoryState,
    pub ram_bytes: u64,
}

/// Partial tab update payload mirroring TypeScript `Partial<TabMetadata>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TabMetadataPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,
    #[serde(rename = "isOAuthPopup", skip_serializing_if = "Option::is_none")]
    pub is_oauth_popup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_awake: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_unsaved_input: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_go_back: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_go_forward: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_loading: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_audible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shield_blocked_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_state: Option<MemoryState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_bytes: Option<u64>,
}

/// Workspace descriptor shared with the Shell UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMetadata {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub profile_name: String,
}

/// Persisted bookmark entry synced to the Shell UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub parent_folder: String,
    pub created_at: i64,
}

/// Single structured block inside a Reading Mode article.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingBlock {
    pub kind: ReadingBlockKind,
    pub text: String,
}

/// Structured Reading Mode article rendered safely as Preact VNodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingArticle {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byline: Option<String>,
    pub blocks: Vec<ReadingBlock>,
}

/// Single Omnibox / Command Palette search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmniboxResultItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub source: OmniboxSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Download status row displayed by the Shell UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String,
    pub file_name: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub state: DownloadState,
}

/// Pending origin permission prompt payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub request_id: String,
    pub origin: String,
    pub kind: PermissionKind,
}

/// Pending JavaScript dialog prompt payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDialogRequest {
    pub request_id: String,
    pub tab_id: String,
    pub kind: ScriptDialogKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_text: Option<String>,
}

/// Pending HTTP Basic Authentication challenge payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasicAuthRequest {
    pub request_id: String,
    pub tab_id: String,
    pub uri: String,
    pub realm: String,
}

/// Pending external URI launch confirmation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUriRequest {
    pub request_id: String,
    pub tab_id: String,
    pub uri: String,
}

/// Extension descriptor for the active workspace toolbar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub has_popup: bool,
    pub has_options_page: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_data_url: Option<String>,
}

/// Pending MCP tool approval prompt payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpActionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub target_origin: String,
    pub summary: String,
}

/// Non-secret vault credential candidate offered for autofill on matching origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAutofillMatch {
    pub credential_id: String,
    pub username_hint: String,
}

/// Non-secret vault entry summary listed in Settings/Vault management.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultCredentialSummary {
    pub credential_id: String,
    pub origin_domain: String,
    pub username_hint: String,
    pub updated_at: i64,
}

/// Frontend Shell -> Backend Rust IPC action messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UIAction {
    // Tab Lifecycle
    #[serde(rename_all = "camelCase")]
    CreateTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_private: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    CloseTab {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    SwitchTab {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    PinTab {
        tab_id: String,
        is_pinned: bool,
    },
    #[serde(rename_all = "camelCase")]
    ToggleKeepAwake {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    DuplicateTab {
        tab_id: String,
    },

    // Navigation
    #[serde(rename_all = "camelCase")]
    Navigate {
        tab_id: String,
        url_or_query: String,
    },
    #[serde(rename_all = "camelCase")]
    GoBack {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    GoForward {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Reload {
        tab_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ignore_cache: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    StopLoad {
        tab_id: String,
    },

    // Workspaces
    #[serde(rename_all = "camelCase")]
    CreateWorkspace {
        name: String,
        icon: String,
    },
    #[serde(rename_all = "camelCase")]
    UpdateWorkspace {
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SwitchWorkspace {
        workspace_id: String,
    },
    #[serde(rename_all = "camelCase")]
    DeleteWorkspace {
        workspace_id: String,
    },

    // Layout, Owned Popup Bounds (Priority Stack), Glance, Split View & Reading Mode
    #[serde(rename_all = "camelCase")]
    SetLayout {
        mode: LayoutMode,
    },
    ToggleZen,
    #[serde(rename_all = "camelCase")]
    SetOverlayBounds {
        visible: bool,
        surface: Option<OverlaySurface>,
        x_dip: f64,
        y_dip: f64,
        width_dip: f64,
        height_dip: f64,
    },
    #[serde(rename_all = "camelCase")]
    SplitView {
        left_tab_id: String,
        right_tab_id: String,
    },
    CloseSplit,
    PromoteGlance,
    CloseGlance,
    #[serde(rename_all = "camelCase")]
    ToggleReadingMode {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    TogglePip {
        tab_id: String,
    },
    WindowMinimize,
    WindowMaximizeRestore,
    WindowClose,

    // Command Palette & Omnibox
    #[serde(rename_all = "camelCase")]
    QueryOmnibox {
        query: String,
    },
    #[serde(rename_all = "camelCase")]
    ExecuteCommand {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
    },
    #[serde(rename_all = "camelCase")]
    OpenInEdge {
        url: String,
    },
    CloseOverlay,

    // Bookmarks CRUD
    #[serde(rename_all = "camelCase")]
    CreateBookmark {
        title: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_folder: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    DeleteBookmark {
        bookmark_id: String,
    },
    ListBookmarks,

    // Settings & Browsing Data
    GetSettings,
    #[serde(rename_all = "camelCase")]
    UpdateSettings {
        patch: AppSettingsPatch,
    },
    #[serde(rename_all = "camelCase")]
    ClearBrowsingData {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        scopes: Vec<ClearScope>,
    },

    // Find-in-Page
    #[serde(rename_all = "camelCase")]
    FindInPage {
        tab_id: String,
        query: String,
        match_case: bool,
        whole_word: bool,
    },
    #[serde(rename_all = "camelCase")]
    FindNext {
        tab_id: String,
        forward: bool,
    },
    #[serde(rename_all = "camelCase")]
    FindClose {
        tab_id: String,
    },

    // Downloads
    #[serde(rename_all = "camelCase")]
    PauseDownload {
        download_id: String,
    },
    #[serde(rename_all = "camelCase")]
    ResumeDownload {
        download_id: String,
    },
    #[serde(rename_all = "camelCase")]
    CancelDownload {
        download_id: String,
    },
    #[serde(rename_all = "camelCase")]
    OpenDownloadFolder {
        download_id: String,
    },

    // Permissions, Script Dialogs, Basic Auth & External URI
    #[serde(rename_all = "camelCase")]
    ResolvePermission {
        request_id: String,
        decision: PermissionDecision,
    },
    #[serde(rename_all = "camelCase")]
    ResolveScriptDialog {
        request_id: String,
        accepted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_text: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ResolveBasicAuth {
        request_id: String,
        cancelled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ResolveExternalUri {
        request_id: String,
        allowed: bool,
    },

    // Shield & Curtain
    #[serde(rename_all = "camelCase")]
    ToggleShield {
        tab_id: String,
        enabled: bool,
    },
    UpdateShieldLists,
    #[serde(rename_all = "camelCase")]
    StartCurtainPicker {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    StopCurtainPicker {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    CurtainHideElement {
        domain: String,
        selector: String,
    },
    #[serde(rename_all = "camelCase")]
    CurtainRemoveRule {
        domain: String,
        selector: String,
    },
    #[serde(rename_all = "camelCase")]
    ListCurtainRules {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<String>,
    },

    // Encrypted Vault
    #[serde(rename_all = "camelCase")]
    VaultCaptureForm {
        tab_id: String,
    },
    #[serde(rename_all = "camelCase")]
    VaultConfirmSave {
        prompt_id: String,
        username: String,
    },
    #[serde(rename_all = "camelCase")]
    VaultUnlockAndFill {
        tab_id: String,
        credential_id: String,
    },
    #[serde(rename_all = "camelCase")]
    VaultSaveManual {
        origin_domain: String,
        username: String,
        password: String,
    },
    VaultListCredentials,
    #[serde(rename_all = "camelCase")]
    VaultDeleteCredential {
        credential_id: String,
    },
    #[serde(rename_all = "camelCase")]
    VaultDismissPrompt {
        prompt_id: String,
    },

    // Extensions
    #[serde(rename_all = "camelCase")]
    LoadUnpackedExtension {
        workspace_id: String,
        folder_path: String,
    },
    #[serde(rename_all = "camelCase")]
    InstallCrxExtension {
        workspace_id: String,
        crx_path: String,
    },
    #[serde(rename_all = "camelCase")]
    ToggleExtensionEnabled {
        workspace_id: String,
        extension_id: String,
        enabled: bool,
    },
    #[serde(rename_all = "camelCase")]
    RemoveExtension {
        workspace_id: String,
        extension_id: String,
    },
    #[serde(rename_all = "camelCase")]
    OpenExtensionPopup {
        workspace_id: String,
        extension_id: String,
        anchor_rect_dip: RectDip,
    },
    #[serde(rename_all = "camelCase")]
    OpenExtensionOptions {
        workspace_id: String,
        extension_id: String,
    },

    // MCP Agent Control
    #[serde(rename_all = "camelCase")]
    SetMcpEnabled {
        enabled: bool,
    },
    #[serde(rename_all = "camelCase")]
    ResolveMcpAction {
        request_id: String,
        decision: McpApprovalDecision,
    },
    StopMcpSession,
}

/// Backend Rust -> Frontend Shell IPC state update events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
// Keep TabMetadata/AppSettings inline rather than Boxed for ergonomic downstream matching.
#[allow(clippy::large_enum_variant)]
pub enum UIEvent {
    #[serde(rename_all = "camelCase")]
    TabCreated { tab: TabMetadata },
    #[serde(rename_all = "camelCase")]
    TabUpdated {
        tab_id: String,
        changes: TabMetadataPatch,
    },
    #[serde(rename_all = "camelCase")]
    TabClosed { tab_id: String },
    #[serde(rename_all = "camelCase")]
    WorkspaceListUpdated {
        active_workspace_id: String,
        workspaces: Vec<WorkspaceMetadata>,
    },
    #[serde(rename_all = "camelCase")]
    MemoryStateChanged {
        tab_id: String,
        state: MemoryState,
        ram_bytes: u64,
    },
    #[serde(rename_all = "camelCase")]
    ShortcutTriggered { action: ShortcutAction },
    #[serde(rename_all = "camelCase")]
    CurtainPickerStateChanged { tab_id: String, active: bool },
    #[serde(rename_all = "camelCase")]
    OmniboxResults { results: Vec<OmniboxResultItem> },
    #[serde(rename_all = "camelCase")]
    BookmarksUpdated { bookmarks: Vec<BookmarkItem> },
    #[serde(rename_all = "camelCase")]
    SettingsUpdated { settings: AppSettings },
    #[serde(rename_all = "camelCase")]
    GlanceStateChanged {
        open: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SplitViewChanged {
        left_tab_id: Option<String>,
        right_tab_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ReadingModeUpdated {
        tab_id: String,
        active: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        article: Option<ReadingArticle>,
    },
    #[serde(rename_all = "camelCase")]
    CurtainRulesUpdated {
        rules: BTreeMap<String, Vec<String>>,
    },
    #[serde(rename_all = "camelCase")]
    ShieldStatusUpdated {
        updating: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_updated: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FindResultUpdated {
        tab_id: String,
        active_match: u32,
        match_count: u32,
    },
    #[serde(rename_all = "camelCase")]
    DownloadProgress { item: DownloadItem },
    #[serde(rename_all = "camelCase")]
    PermissionRequested { req: PermissionRequest },
    #[serde(rename_all = "camelCase")]
    ScriptDialogOpening { req: ScriptDialogRequest },
    #[serde(rename_all = "camelCase")]
    BasicAuthRequested { req: BasicAuthRequest },
    #[serde(rename_all = "camelCase")]
    ExternalUriRequested { req: ExternalUriRequest },
    #[serde(rename_all = "camelCase")]
    VaultAutofillAvailable {
        tab_id: String,
        origin_domain: String,
        matches: Vec<VaultAutofillMatch>,
    },
    #[serde(rename_all = "camelCase")]
    VaultPromptSave {
        prompt_id: String,
        tab_id: String,
        origin_domain: String,
        username: String,
    },
    #[serde(rename_all = "camelCase")]
    VaultCredentialsListed { items: Vec<VaultCredentialSummary> },
    #[serde(rename_all = "camelCase")]
    VaultAuthResult {
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ExtensionsUpdated {
        workspace_id: String,
        extensions: Vec<ExtensionMetadata>,
    },
    #[serde(rename_all = "camelCase")]
    McpStatusChanged { enabled: bool, active_client: bool },
    #[serde(rename_all = "camelCase")]
    McpActionRequested { req: McpActionRequest },
    #[serde(rename_all = "camelCase")]
    RuntimeUpdateAvailable { version: String },
    #[serde(rename_all = "camelCase")]
    BrowserProcessCrashed { reason: String, recovering: bool },
}

/// Origin-locked WebMessage sent from `chrome-extension://{id}/` documents (`chrome.tabs` shim).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExtensionShimMessage {
    #[serde(rename_all = "camelCase")]
    ExtCreateTab { url: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn sample_tab_metadata(is_oauth_popup: bool) -> TabMetadata {
        TabMetadata {
            id: "tab-101".to_string(),
            workspace_id: "ws-default".to_string(),
            title: "Sign in".to_string(),
            url: "https://accounts.example.com/oauth".to_string(),
            favicon_url: "https://accounts.example.com/favicon.ico".to_string(),
            is_pinned: false,
            is_private: false,
            is_oauth_popup,
            keep_awake: false,
            has_unsaved_input: false,
            can_go_back: true,
            can_go_forward: false,
            is_loading: false,
            is_audible: false,
            shield_blocked_count: 3,
            memory_state: MemoryState::Active,
            ram_bytes: 42_000_000,
        }
    }

    #[test]
    fn app_settings_default_matches_v3_3_spec() {
        let settings = AppSettings::default();
        assert_eq!(settings.version, "3.3");
        assert_eq!(settings.layout_mode, LayoutMode::VerticalExpanded);
        assert!(!settings.zen_mode);
        assert_eq!(settings.v8_heap_profile, V8HeapProfile::Balanced);
        assert_eq!(settings.lru_active_cap, 4);
        assert_eq!(settings.soft_sleep_timeout_sec, 180);
        assert_eq!(settings.hard_unload_timeout_sec, 1500);
        assert_eq!(
            settings.search_engine_template,
            "https://duckduckgo.com/?q=%s"
        );
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert!(!settings.shield_auto_update);
        assert!(!settings.mcp_enabled);

        let serialized = serde_json::to_value(&settings).expect("serialize AppSettings");
        assert_eq!(
            serialized,
            json!({
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
            })
        );

        let partial: AppSettings = serde_json::from_str(r#"{"zen_mode":true}"#)
            .expect("partial settings.json should default missing fields");
        assert!(partial.zen_mode);
        assert_eq!(partial.version, "3.3");
        assert!(!partial.shield_auto_update);
    }

    #[test]
    fn ui_action_set_overlay_bounds_round_trip() {
        let action = UIAction::SetOverlayBounds {
            visible: true,
            surface: Some(OverlaySurface::CommandPalette),
            x_dip: 120.5,
            y_dip: 48.0,
            width_dip: 640.0,
            height_dip: 420.0,
        };
        let value = serde_json::to_value(&action).expect("serialize SetOverlayBounds");
        assert_eq!(
            value,
            json!({
                "type": "SET_OVERLAY_BOUNDS",
                "visible": true,
                "surface": "command_palette",
                "xDip": 120.5,
                "yDip": 48.0,
                "widthDip": 640.0,
                "heightDip": 420.0
            })
        );
        let decoded: UIAction =
            serde_json::from_value(value).expect("deserialize SetOverlayBounds");
        assert_eq!(decoded, action);

        let hidden = UIAction::SetOverlayBounds {
            visible: false,
            surface: None,
            x_dip: 0.0,
            y_dip: 0.0,
            width_dip: 0.0,
            height_dip: 0.0,
        };
        let hidden_val = serde_json::to_value(&hidden).expect("serialize hidden SetOverlayBounds");
        assert_eq!(hidden_val.get("surface"), Some(&Value::Null));
        let hidden_decoded: UIAction =
            serde_json::from_value(hidden_val).expect("deserialize hidden SetOverlayBounds");
        assert_eq!(hidden_decoded, hidden);
    }

    #[test]
    fn ui_action_clear_browsing_data_round_trip() {
        let scoped = UIAction::ClearBrowsingData {
            workspace_id: Some("ws-default".to_string()),
            scopes: vec![ClearScope::Cache, ClearScope::Cookies, ClearScope::History],
        };
        let val = serde_json::to_value(&scoped).expect("serialize ClearBrowsingData");
        assert_eq!(
            val,
            json!({
                "type": "CLEAR_BROWSING_DATA",
                "workspaceId": "ws-default",
                "scopes": ["cache", "cookies", "history"]
            })
        );
        let decoded: UIAction = serde_json::from_value(val).expect("deserialize ClearBrowsingData");
        assert_eq!(decoded, scoped);

        let global_json = r#"{"type":"CLEAR_BROWSING_DATA","scopes":["history"]}"#;
        let global: UIAction = serde_json::from_str(global_json).expect("deserialize global clear");
        assert_eq!(
            global,
            UIAction::ClearBrowsingData {
                workspace_id: None,
                scopes: vec![ClearScope::History],
            }
        );
    }

    #[test]
    fn ui_action_update_settings_round_trip_and_apply_patch() {
        let raw = r#"{"type":"UPDATE_SETTINGS","patch":{"zen_mode":true,"theme":"light","layout_mode":"topbar"}}"#;
        let action: UIAction = serde_json::from_str(raw).expect("deserialize UpdateSettings");
        let expected = UIAction::UpdateSettings {
            patch: AppSettingsPatch {
                zen_mode: Some(true),
                theme: Some(ThemeMode::Light),
                layout_mode: Some(LayoutMode::Topbar),
                ..Default::default()
            },
        };
        assert_eq!(action, expected);

        let reserialized = serde_json::to_value(&action).expect("serialize UpdateSettings");
        assert_eq!(
            reserialized,
            json!({
                "type": "UPDATE_SETTINGS",
                "patch": {
                    "layout_mode": "topbar",
                    "zen_mode": true,
                    "theme": "light"
                }
            })
        );

        if let UIAction::UpdateSettings { patch } = action {
            let mut settings = AppSettings::default();
            settings.apply_patch(patch);
            assert!(settings.zen_mode);
            assert_eq!(settings.theme, ThemeMode::Light);
            assert_eq!(settings.layout_mode, LayoutMode::Topbar);
            assert_eq!(settings.version, "3.3");
            assert_eq!(settings.lru_active_cap, 4);
        } else {
            panic!("expected UpdateSettings");
        }
    }

    #[test]
    fn ui_event_tab_created_preserves_is_oauth_popup_casing() {
        let event = UIEvent::TabCreated {
            tab: sample_tab_metadata(true),
        };
        let value = serde_json::to_value(&event).expect("serialize TabCreated");
        assert_eq!(value["type"], "TAB_CREATED");
        assert_eq!(value["tab"]["isOAuthPopup"], Value::Bool(true));
        assert!(value["tab"].get("isOauthPopup").is_none());
        assert_eq!(value["tab"]["memoryState"], "active");

        let decoded: UIEvent = serde_json::from_value(value).expect("deserialize TabCreated");
        assert_eq!(decoded, event);
    }

    #[test]
    fn ui_event_vault_prompt_save_round_trip_and_zero_password() {
        let event = UIEvent::VaultPromptSave {
            prompt_id: "prompt-1".to_string(),
            tab_id: "tab-101".to_string(),
            origin_domain: "https://github.com".to_string(),
            username: "octocat".to_string(),
        };
        let value = serde_json::to_value(&event).expect("serialize VaultPromptSave");
        assert_eq!(
            value,
            json!({
                "type": "VAULT_PROMPT_SAVE",
                "promptId": "prompt-1",
                "tabId": "tab-101",
                "originDomain": "https://github.com",
                "username": "octocat"
            })
        );
        assert!(value.get("password").is_none());

        let decoded: UIEvent = serde_json::from_value(value).expect("deserialize VaultPromptSave");
        assert_eq!(decoded, event);
    }

    #[test]
    fn ui_event_shortcut_triggered_round_trip() {
        for (action, expected_str) in [
            (ShortcutAction::OpenCommandPalette, "open_command_palette"),
            (ShortcutAction::OpenFindInPage, "open_find_in_page"),
            (ShortcutAction::CloseActiveOverlay, "close_active_overlay"),
        ] {
            let event = UIEvent::ShortcutTriggered { action };
            let value = serde_json::to_value(&event).expect("serialize ShortcutTriggered");
            assert_eq!(
                value,
                json!({
                    "type": "SHORTCUT_TRIGGERED",
                    "action": expected_str
                })
            );
            let decoded: UIEvent =
                serde_json::from_value(value).expect("deserialize ShortcutTriggered");
            assert_eq!(decoded, event);
        }
    }

    #[test]
    fn extension_shim_message_ext_create_tab_round_trip() {
        let msg = ExtensionShimMessage::ExtCreateTab {
            url: "https://nyari.example/welcome".to_string(),
        };
        let value = serde_json::to_value(&msg).expect("serialize ExtensionShimMessage");
        assert_eq!(
            value,
            json!({
                "type": "EXT_CREATE_TAB",
                "url": "https://nyari.example/welcome"
            })
        );
        let decoded: ExtensionShimMessage =
            serde_json::from_value(value).expect("deserialize ExtensionShimMessage");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn ipc_enums_and_complex_variants_round_trip() {
        let layout_action = UIAction::SetLayout {
            mode: LayoutMode::VerticalCollapsed,
        };
        assert_eq!(
            serde_json::to_value(&layout_action).unwrap(),
            json!({"type": "SET_LAYOUT", "mode": "vertical-collapsed"})
        );

        let popup_action = UIAction::OpenExtensionPopup {
            workspace_id: "ws-work".to_string(),
            extension_id: "cjpalhdlnbpafiamejdnhcphjbkeiagm".to_string(),
            anchor_rect_dip: RectDip {
                x: 12.0,
                y: 44.0,
                width: 28.0,
                height: 28.0,
            },
        };
        let popup_val = serde_json::to_value(&popup_action).unwrap();
        assert_eq!(
            serde_json::from_value::<UIAction>(popup_val).unwrap(),
            popup_action
        );

        let mcp_action = UIAction::ResolveMcpAction {
            request_id: "mcp-1".to_string(),
            decision: McpApprovalDecision::AllowOriginSession,
        };
        let mcp_val = serde_json::to_value(&mcp_action).unwrap();
        assert_eq!(mcp_val["decision"], "allow_origin_session");
        assert_eq!(
            serde_json::from_value::<UIAction>(mcp_val).unwrap(),
            mcp_action
        );

        let perm_action = UIAction::ResolvePermission {
            request_id: "perm-1".to_string(),
            decision: PermissionDecision::Allow,
        };
        let perm_val = serde_json::to_value(&perm_action).unwrap();
        assert_eq!(perm_val["decision"], "allow");
        assert_eq!(
            serde_json::from_value::<UIAction>(perm_val).unwrap(),
            perm_action
        );

        let split_evt = UIEvent::SplitViewChanged {
            left_tab_id: Some("tab-1".to_string()),
            right_tab_id: None,
        };
        let split_val = serde_json::to_value(&split_evt).unwrap();
        assert_eq!(
            split_val,
            json!({
                "type": "SPLIT_VIEW_CHANGED",
                "leftTabId": "tab-1",
                "rightTabId": null
            })
        );
        assert_eq!(
            serde_json::from_value::<UIEvent>(split_val).unwrap(),
            split_evt
        );

        let tab_patch = UIEvent::TabUpdated {
            tab_id: "tab-101".to_string(),
            changes: TabMetadataPatch {
                is_oauth_popup: Some(true),
                memory_state: Some(MemoryState::SoftSleepDegraded),
                ..Default::default()
            },
        };
        let tab_patch_val = serde_json::to_value(&tab_patch).unwrap();
        assert_eq!(
            tab_patch_val,
            json!({
                "type": "TAB_UPDATED",
                "tabId": "tab-101",
                "changes": {
                    "isOAuthPopup": true,
                    "memoryState": "soft_sleep_degraded"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<UIEvent>(tab_patch_val).unwrap(),
            tab_patch
        );

        let reading_evt = UIEvent::ReadingModeUpdated {
            tab_id: "tab-101".to_string(),
            active: true,
            article: Some(ReadingArticle {
                title: "Nyari".to_string(),
                byline: None,
                blocks: vec![ReadingBlock {
                    kind: ReadingBlockKind::Blockquote,
                    text: "Fast local browser".to_string(),
                }],
            }),
        };
        let reading_val = serde_json::to_value(&reading_evt).unwrap();
        assert_eq!(
            serde_json::from_value::<UIEvent>(reading_val).unwrap(),
            reading_evt
        );

        let vault_evt = UIEvent::VaultAutofillAvailable {
            tab_id: "tab-1".to_string(),
            origin_domain: "https://example.com".to_string(),
            matches: vec![VaultAutofillMatch {
                credential_id: "cred-1".to_string(),
                username_hint: "alice@example.com".to_string(),
            }],
        };
        let vault_val = serde_json::to_value(&vault_evt).unwrap();
        assert_eq!(
            serde_json::from_value::<UIEvent>(vault_val).unwrap(),
            vault_evt
        );

        let creds_evt = UIEvent::VaultCredentialsListed {
            items: vec![VaultCredentialSummary {
                credential_id: "cred-1".to_string(),
                origin_domain: "https://example.com".to_string(),
                username_hint: "alice@example.com".to_string(),
                updated_at: 1_700_000_000_000,
            }],
        };
        let creds_val = serde_json::to_value(&creds_evt).unwrap();
        assert_eq!(
            serde_json::from_value::<UIEvent>(creds_val).unwrap(),
            creds_evt
        );
    }
}
