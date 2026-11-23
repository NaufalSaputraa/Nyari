import { computed, effect, signal } from "@preact/signals";

export type LayoutMode = "vertical-expanded" | "vertical-collapsed" | "topbar";
export type OverlaySurface = "modal" | "command_palette" | "reading_mode" | "hover_sidebar";
export type MemoryState = "active" | "soft_sleep" | "soft_sleep_degraded" | "hard_unload";
export type ThemeMode = "dark" | "light";
export type V8HeapProfile = "eco" | "balanced" | "unlimited";

export interface AppSettings {
  version: string;
  layout_mode: LayoutMode;
  zen_mode: boolean;
  v8_heap_profile: V8HeapProfile;
  lru_active_cap: number;
  soft_sleep_timeout_sec: number;
  hard_unload_timeout_sec: number;
  search_engine_template: string;
  theme: ThemeMode;
  shield_auto_update: boolean;
  mcp_enabled: boolean;
}

export interface TabMetadata {
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
  memoryState: MemoryState;
  ramBytes: number;
}

export interface WorkspaceMetadata {
  id: string;
  name: string;
  icon: string;
  profileName: string;
}

export interface OmniboxResultItem {
  id: string;
  title: string;
  url: string;
  source: "tab" | "history" | "bookmark" | "command" | "search";
  icon?: string;
}

export interface ReadingBlock {
  kind: "h1" | "h2" | "p" | "blockquote" | "pre" | "li";
  text: string;
}

export interface ReadingArticle {
  title: string;
  byline?: string;
  blocks: ReadingBlock[];
}

export interface PermissionRequest {
  requestId: string;
  origin: string;
  kind: "camera" | "microphone" | "geolocation" | "notifications" | "clipboard" | "screen_capture";
}

export interface ScriptDialogRequest {
  requestId: string;
  tabId: string;
  kind: "alert" | "confirm" | "prompt" | "beforeunload";
  message: string;
  defaultText?: string;
}

export interface BasicAuthRequest {
  requestId: string;
  tabId: string;
  uri: string;
  realm: string;
}

export interface ExternalUriRequest {
  requestId: string;
  tabId: string;
  uri: string;
}

export interface McpActionRequest {
  requestId: string;
  toolName: string;
  targetOrigin: string;
  summary: string;
}

export type PendingModal =
  | { kind: "permission"; req: PermissionRequest }
  | { kind: "script_dialog"; req: ScriptDialogRequest }
  | { kind: "basic_auth"; req: BasicAuthRequest }
  | { kind: "external_uri"; req: ExternalUriRequest }
  | { kind: "mcp_action"; req: McpActionRequest };

export type UIAction =
  | { type: "CREATE_TAB"; url?: string; workspaceId: string; isPrivate?: boolean }
  | { type: "CLOSE_TAB"; tabId: string }
  | { type: "SWITCH_TAB"; tabId: string }
  | { type: "PIN_TAB"; tabId: string; isPinned: boolean }
  | { type: "TOGGLE_KEEP_AWAKE"; tabId: string }
  | { type: "NAVIGATE"; tabId: string; urlOrQuery: string }
  | { type: "GO_BACK"; tabId: string }
  | { type: "GO_FORWARD"; tabId: string }
  | { type: "RELOAD"; tabId: string; ignoreCache?: boolean }
  | { type: "SWITCH_WORKSPACE"; workspaceId: string }
  | { type: "SET_LAYOUT"; mode: LayoutMode }
  | { type: "TOGGLE_ZEN" }
  | {
      type: "SET_OVERLAY_BOUNDS";
      visible: boolean;
      surface: OverlaySurface | null;
      xDip: number;
      yDip: number;
      widthDip: number;
      heightDip: number;
    }
  | { type: "TOGGLE_READING_MODE"; tabId: string }
  | { type: "WINDOW_MINIMIZE" }
  | { type: "WINDOW_MAXIMIZE_RESTORE" }
  | { type: "WINDOW_CLOSE" }
  | { type: "QUERY_OMNIBOX"; query: string }
  | { type: "EXECUTE_COMMAND"; command: string; args?: string[] }
  | { type: "OPEN_IN_EDGE"; url: string }
  | { type: "CLOSE_OVERLAY" }
  | { type: "GET_SETTINGS" }
  | { type: "UPDATE_SETTINGS"; patch: Partial<AppSettings> }
  | { type: "FIND_IN_PAGE"; tabId: string; query: string; matchCase: boolean; wholeWord: boolean }
  | { type: "FIND_NEXT"; tabId: string; forward: boolean }
  | { type: "FIND_CLOSE"; tabId: string }
  | { type: "RESOLVE_PERMISSION"; requestId: string; decision: "allow" | "deny" }
  | { type: "RESOLVE_SCRIPT_DIALOG"; requestId: string; accepted: boolean; promptText?: string }
  | {
      type: "RESOLVE_BASIC_AUTH";
      requestId: string;
      cancelled: boolean;
      username?: string;
      password?: string;
    }
  | { type: "RESOLVE_EXTERNAL_URI"; requestId: string; allowed: boolean }
  | {
      type: "RESOLVE_MCP_ACTION";
      requestId: string;
      decision: "allow_once" | "allow_origin_session" | "deny";
    };

export type UIEvent =
  | { type: "TAB_CREATED"; tab: TabMetadata }
  | { type: "TAB_UPDATED"; tabId: string; changes: Partial<TabMetadata> }
  | { type: "TAB_CLOSED"; tabId: string }
  | { type: "WORKSPACE_LIST_UPDATED"; activeWorkspaceId: string; workspaces: WorkspaceMetadata[] }
  | { type: "MEMORY_STATE_CHANGED"; tabId: string; state: MemoryState; ramBytes: number }
  | {
      type: "SHORTCUT_TRIGGERED";
      action: "open_command_palette" | "open_find_in_page" | "close_active_overlay";
    }
  | { type: "OMNIBOX_RESULTS"; results: OmniboxResultItem[] }
  | { type: "SETTINGS_UPDATED"; settings: AppSettings }
  | { type: "READING_MODE_UPDATED"; tabId: string; active: boolean; article?: ReadingArticle }
  | { type: "FIND_RESULT_UPDATED"; tabId: string; activeMatch: number; matchCount: number }
  | { type: "PERMISSION_REQUESTED"; req: PermissionRequest }
  | { type: "SCRIPT_DIALOG_OPENING"; req: ScriptDialogRequest }
  | { type: "BASIC_AUTH_REQUESTED"; req: BasicAuthRequest }
  | { type: "EXTERNAL_URI_REQUESTED"; req: ExternalUriRequest }
  | { type: "MCP_ACTION_REQUESTED"; req: McpActionRequest }
  | { type: "RUNTIME_UPDATE_AVAILABLE"; version: string }
  | { type: "BROWSER_PROCESS_CRASHED"; reason: string; recovering: boolean };

interface WebView2Bridge {
  postMessage(message: string): void;
  addEventListener(type: "message", listener: (event: MessageEvent<unknown>) => void): void;
}

declare global {
  interface Window {
    chrome?: {
      webview?: WebView2Bridge;
    };
    __NYARI_VIEWPORT__?: {
      widthDip: number;
      heightDip: number;
    };
    __NYARI_DISMISS_TRANSIENT__?: (clearCmd: boolean, clearHover: boolean) => void;
    __NYARI_SET_ACTIVE_TAB__?: (tabId: string | null) => void;
  }
}

export const defaultSettings: AppSettings = {
  version: "3.3",
  layout_mode: "vertical-expanded",
  zen_mode: false,
  v8_heap_profile: "balanced",
  lru_active_cap: 4,
  soft_sleep_timeout_sec: 180,
  hard_unload_timeout_sec: 1500,
  search_engine_template: "https://duckduckgo.com/?q=%s",
  theme: "dark",
  shield_auto_update: false,
  mcp_enabled: false
};

export const settings = signal<AppSettings>(defaultSettings);
export const tabs = signal<TabMetadata[]>([]);
export const activeTabId = signal<string | null>(null);
export const workspaces = signal<WorkspaceMetadata[]>([
  { id: "ws-default", name: "Default", icon: "D", profileName: "nyari-ws-default" }
]);
export const activeWorkspaceId = signal<string>("ws-default");

export const commandPaletteOpen = signal<boolean>(false);
export const findInPageOpen = signal<boolean>(false);
export const findQuery = signal<string>("");
export const findMatchInfo = signal<{ activeMatch: number; matchCount: number }>({
  activeMatch: 0,
  matchCount: 0
});
export const readingModeOpen = signal<boolean>(false);
export const readingArticle = signal<ReadingArticle | null>(null);
export const hoverSidebarOpen = signal<boolean>(false);
export const modalQueue = signal<PendingModal[]>([]);
export const omniboxResults = signal<OmniboxResultItem[]>([]);
export const runtimeUpdateVersion = signal<string | null>(null);
export const browserCrashReason = signal<string | null>(null);

export const viewportSize = signal<{ widthDip: number; heightDip: number }>({
  widthDip: window.__NYARI_VIEWPORT__?.widthDip ?? 1280,
  heightDip: window.__NYARI_VIEWPORT__?.heightDip ?? 820
});

export const activeWorkspaceTabs = computed(() =>
  tabs.value.filter((tab) => tab.workspaceId === activeWorkspaceId.value)
);

export const activeTab = computed(
  () => tabs.value.find((tab) => tab.id === activeTabId.value) ?? null
);

export const activeOverlaySurface = computed<OverlaySurface | null>(() => {
  if (modalQueue.value.length > 0) return "modal";
  if (commandPaletteOpen.value || findInPageOpen.value) return "command_palette";
  if (readingModeOpen.value) return "reading_mode";
  if (hoverSidebarOpen.value) return "hover_sidebar";
  return null;
});

effect(() => {
  document.documentElement.setAttribute("data-theme", settings.value.theme);
});

export function sendAction(action: UIAction): void {
  const webview = window.chrome?.webview;
  if (webview) {
    webview.postMessage(JSON.stringify(action));
  }
}

export function switchTab(tabId: string): void {
  activeTabId.value = tabId;
  sendAction({ type: "SWITCH_TAB", tabId });
}

export function popModal(requestId: string): void {
  modalQueue.value = modalQueue.value.filter((item) => item.req.requestId !== requestId);
}

function handleUIEvent(ev: UIEvent): void {
  switch (ev.type) {
    case "SETTINGS_UPDATED":
      settings.value = ev.settings;
      break;
    case "TAB_CREATED":
      if (!tabs.value.some((t) => t.id === ev.tab.id)) {
        tabs.value = [...tabs.value, ev.tab];
      }
      activeTabId.value = ev.tab.id;
      break;
    case "TAB_UPDATED":
      tabs.value = tabs.value.map((t) => (t.id === ev.tabId ? { ...t, ...ev.changes } : t));
      break;
    case "TAB_CLOSED": {
      const remaining = tabs.value.filter((t) => t.id !== ev.tabId);
      tabs.value = remaining;
      if (activeTabId.value === ev.tabId) {
        activeTabId.value = remaining[0]?.id ?? null;
      }
      break;
    }
    case "WORKSPACE_LIST_UPDATED":
      workspaces.value = ev.workspaces;
      activeWorkspaceId.value = ev.activeWorkspaceId;
      break;
    case "MEMORY_STATE_CHANGED":
      tabs.value = tabs.value.map((t) =>
        t.id === ev.tabId ? { ...t, memoryState: ev.state, ramBytes: ev.ramBytes } : t
      );
      break;
    case "SHORTCUT_TRIGGERED":
      if (ev.action === "open_command_palette") {
        findInPageOpen.value = false;
        commandPaletteOpen.value = true;
      } else if (ev.action === "open_find_in_page") {
        commandPaletteOpen.value = false;
        findInPageOpen.value = true;
      } else if (ev.action === "close_active_overlay") {
        if (commandPaletteOpen.value) {
          commandPaletteOpen.value = false;
        } else if (findInPageOpen.value) {
          findInPageOpen.value = false;
        } else if (hoverSidebarOpen.value) {
          hoverSidebarOpen.value = false;
        } else if (readingModeOpen.value) {
          readingModeOpen.value = false;
        }
      }
      break;
    case "OMNIBOX_RESULTS":
      omniboxResults.value = ev.results;
      break;
    case "READING_MODE_UPDATED":
      readingModeOpen.value = ev.active;
      readingArticle.value = ev.article ?? null;
      break;
    case "FIND_RESULT_UPDATED":
      findMatchInfo.value = { activeMatch: ev.activeMatch, matchCount: ev.matchCount };
      break;
    case "PERMISSION_REQUESTED":
      modalQueue.value = [...modalQueue.value, { kind: "permission", req: ev.req }];
      break;
    case "SCRIPT_DIALOG_OPENING":
      modalQueue.value = [...modalQueue.value, { kind: "script_dialog", req: ev.req }];
      break;
    case "BASIC_AUTH_REQUESTED":
      modalQueue.value = [...modalQueue.value, { kind: "basic_auth", req: ev.req }];
      break;
    case "EXTERNAL_URI_REQUESTED":
      modalQueue.value = [...modalQueue.value, { kind: "external_uri", req: ev.req }];
      break;
    case "MCP_ACTION_REQUESTED":
      modalQueue.value = [...modalQueue.value, { kind: "mcp_action", req: ev.req }];
      break;
    case "RUNTIME_UPDATE_AVAILABLE":
      runtimeUpdateVersion.value = ev.version;
      break;
    case "BROWSER_PROCESS_CRASHED":
      browserCrashReason.value = ev.recovering ? null : ev.reason;
      break;
  }
}

let bridgeInitialized = false;

export function initIpcBridge(): void {
  if (bridgeInitialized) return;
  bridgeInitialized = true;

  window.__NYARI_DISMISS_TRANSIENT__ = (clearCmd: boolean, clearHover: boolean) => {
    if (clearCmd) commandPaletteOpen.value = false;
    if (clearHover) hoverSidebarOpen.value = false;
  };

  window.__NYARI_SET_ACTIVE_TAB__ = (tabId: string | null) => {
    activeTabId.value = tabId;
  };

  window.addEventListener("nyari-viewport", () => {
    if (window.__NYARI_VIEWPORT__) {
      viewportSize.value = { ...window.__NYARI_VIEWPORT__ };
    }
  });

  window.chrome?.webview?.addEventListener("message", (event) => {
    const raw = event.data;
    try {
      const parsed = typeof raw === "string" ? (JSON.parse(raw) as UIEvent) : (raw as UIEvent);
      if (parsed && typeof parsed.type === "string") {
        handleUIEvent(parsed);
      }
    } catch {
      // Ignore malformed IPC payloads from non-JSON callers.
    }
  });

  sendAction({ type: "GET_SETTINGS" });
}
