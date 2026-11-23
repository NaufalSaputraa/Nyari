import { activeTab, activeTabId, activeWorkspaceId, activeWorkspaceTabs, sendAction, settings } from "./store";

export function WindowControls() {
  return (
    <div class="window-controls" aria-label="Window controls">
      <button
        type="button"
        class="win-btn"
        aria-label="Minimize window"
        onClick={() => sendAction({ type: "WINDOW_MINIMIZE" })}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <line x1="1" y1="5" x2="9" y2="5" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
      <button
        type="button"
        class="win-btn"
        aria-label="Maximize or restore window"
        onClick={() => sendAction({ type: "WINDOW_MAXIMIZE_RESTORE" })}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
      <button
        type="button"
        class="win-btn win-btn--close"
        aria-label="Close window"
        onClick={() => sendAction({ type: "WINDOW_CLOSE" })}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <path d="M2 2l6 6M8 2l-6 6" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    </div>
  );
}

export function TopBar() {
  const current = activeTab.value;
  const wsTabs = activeWorkspaceTabs.value;
  const currentTheme = settings.value.theme;

  return (
    <header class="topbar" aria-label="Top navigation bar">
      <div class="topbar-nav">
        <button
          type="button"
          class="icon-btn"
          aria-label="Back"
          disabled={!current?.canGoBack}
          onClick={() => current && sendAction({ type: "GO_BACK", tabId: current.id })}
        >
          &lt;
        </button>
        <button
          type="button"
          class="icon-btn"
          aria-label="Forward"
          disabled={!current?.canGoForward}
          onClick={() => current && sendAction({ type: "GO_FORWARD", tabId: current.id })}
        >
          &gt;
        </button>
        <button
          type="button"
          class="icon-btn"
          aria-label="Reload"
          disabled={!current}
          onClick={() => current && sendAction({ type: "RELOAD", tabId: current.id })}
        >
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path
              d="M13 8a5 5 0 1 1-1.3-3.4M13 2.5V5.5H10"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>

      <div class="topbar-tabs" role="tablist" aria-label="Open tabs">
        {wsTabs.map((tab) => {
          const isActive = tab.id === activeTabId.value;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={isActive}
              class={isActive ? "topbar-tab-chip topbar-tab-chip--active" : "topbar-tab-chip"}
              onClick={() => sendAction({ type: "SWITCH_TAB", tabId: tab.id })}
            >
              <span class="tab-title">{tab.title || tab.url || "New Tab"}</span>
            </button>
          );
        })}
        <button
          type="button"
          class="icon-btn"
          aria-label="New tab"
          onClick={() =>
            sendAction({ type: "CREATE_TAB", workspaceId: activeWorkspaceId.value })
          }
        >
          +
        </button>
      </div>

      <div class="topbar-tools">
        <button
          type="button"
          class="address-pill"
          aria-label="Open Command Palette"
          onClick={() =>
            sendAction({ type: "EXECUTE_COMMAND", command: "open_command_palette" })
          }
        >
          <span>{current?.url || "Search or enter URL (Ctrl+K)"}</span>
        </button>
        <button
          type="button"
          class="icon-btn"
          aria-label="Switch to Vertical Sidebar layout"
          title="Vertical Sidebar layout"
          onClick={() => sendAction({ type: "SET_LAYOUT", mode: "vertical-expanded" })}
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <rect
              x="2"
              y="2.5"
              width="12"
              height="11"
              rx="2"
              stroke="currentColor"
              stroke-width="1.4"
            />
            <line x1="6" y1="2.5" x2="6" y2="13.5" stroke="currentColor" stroke-width="1.4" />
          </svg>
        </button>
        <button
          type="button"
          class="icon-btn"
          aria-label={`Switch to ${currentTheme === "dark" ? "light" : "dark"} theme`}
          title="Toggle theme"
          onClick={() =>
            sendAction({
              type: "UPDATE_SETTINGS",
              patch: { theme: currentTheme === "dark" ? "light" : "dark" }
            })
          }
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="8" cy="8" r="3.5" stroke="currentColor" stroke-width="1.4" />
          </svg>
        </button>
      </div>

      <WindowControls />
    </header>
  );
}
