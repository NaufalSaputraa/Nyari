import {
  activeTabId,
  activeWorkspaceId,
  activeWorkspaceTabs,
  runtimeUpdateVersion,
  sendAction,
  settings,
  switchTab,
  workspaces
} from "./store";

interface SidebarProps {
  collapsed: boolean;
  isHoverOverlay?: boolean;
  onCloseOverlay?: () => void;
}

export function Sidebar({ collapsed, isHoverOverlay = false, onCloseOverlay }: SidebarProps) {
  const wsTabs = activeWorkspaceTabs.value;
  const pinnedTabs = wsTabs.filter((t) => t.isPinned);
  const regularTabs = wsTabs.filter((t) => !t.isPinned);
  const currentTabId = activeTabId.value;
  const currentTheme = settings.value.theme;
  const currentLayout = settings.value.layout_mode;

  const createTab = (isPrivate = false) => {
    sendAction({
      type: "CREATE_TAB",
      workspaceId: activeWorkspaceId.value,
      isPrivate
    });
    if (isHoverOverlay && onCloseOverlay) onCloseOverlay();
  };

  const toggleCollapse = () => {
    const nextMode =
      currentLayout === "vertical-collapsed" ? "vertical-expanded" : "vertical-collapsed";
    sendAction({ type: "SET_LAYOUT", mode: nextMode });
  };

  const toggleTopbar = () => {
    sendAction({ type: "SET_LAYOUT", mode: "topbar" });
  };

  const toggleTheme = () => {
    sendAction({
      type: "UPDATE_SETTINGS",
      patch: { theme: currentTheme === "dark" ? "light" : "dark" }
    });
  };

  return (
    <aside
      class={
        isHoverOverlay
          ? "sidebar sidebar--overlay"
          : collapsed
            ? "sidebar sidebar--collapsed"
            : "sidebar"
      }
      aria-label="Workspace and tab navigation"
    >
      <div class="sidebar-header">
        {!collapsed ? (
          <div class="sidebar-brand">
            <span>Nyari</span>
          </div>
        ) : null}
        {!isHoverOverlay ? (
          <button
            type="button"
            class="icon-btn"
            aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            aria-expanded={!collapsed}
            title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            onClick={toggleCollapse}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
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
        ) : (
          <button
            type="button"
            class="icon-btn"
            aria-label="Close sidebar drawer"
            onClick={onCloseOverlay}
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
              <path
                d="M3 3l8 8M11 3l-8 8"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            </svg>
          </button>
        )}
      </div>

      <nav
        class={collapsed ? "workspace-strip workspace-strip--collapsed" : "workspace-strip"}
        aria-label="Workspaces"
      >
        {workspaces.value.map((ws) => {
          const active = ws.id === activeWorkspaceId.value;
          return (
            <button
              key={ws.id}
              type="button"
              class={active ? "workspace-chip workspace-chip--active" : "workspace-chip"}
              aria-pressed={active}
              title={ws.name}
              onClick={() => sendAction({ type: "SWITCH_WORKSPACE", workspaceId: ws.id })}
            >
              {collapsed ? ws.icon : `${ws.icon} ${ws.name}`}
            </button>
          );
        })}
      </nav>

      {pinnedTabs.length > 0 ? (
        <div class={collapsed ? "workspace-strip workspace-strip--collapsed" : "pinned-grid"}>
          {pinnedTabs.map((tab) => {
            const isActive = tab.id === currentTabId;
            return (
              <button
                key={tab.id}
                type="button"
                class={isActive ? "pinned-tile pinned-tile--active" : "pinned-tile"}
                title={tab.title || tab.url}
                aria-label={`Pinned tab ${tab.title || tab.url}`}
                onClick={() => switchTab(tab.id)}
              >
                {(tab.title || "T").slice(0, 1).toUpperCase()}
              </button>
            );
          })}
        </div>
      ) : null}

      <div class="tab-list" role="tablist" aria-label="Open tabs">
        {regularTabs.length === 0 ? (
          collapsed ? (
            <button
              type="button"
              class="icon-btn"
              aria-label="New tab"
              title="New tab"
              onClick={() => createTab(false)}
            >
              +
            </button>
          ) : (
            <div class="empty-tabs">
              <span>No open tabs in this workspace.</span>
              <button type="button" class="primary-btn" onClick={() => createTab(false)}>
                New Tab
              </button>
            </div>
          )
        ) : (
          regularTabs.map((tab) => {
            const isActive = tab.id === currentTabId;
            const isSleeping =
              tab.memoryState === "soft_sleep" ||
              tab.memoryState === "soft_sleep_degraded" ||
              tab.memoryState === "hard_unload";
            return (
              <div
                key={tab.id}
                class={isActive ? "tab-row tab-row--active" : "tab-row"}
                role="presentation"
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  class="tab-main-btn"
                  title={tab.title || tab.url}
                  onClick={() => {
                    switchTab(tab.id);
                    if (isHoverOverlay && onCloseOverlay) onCloseOverlay();
                  }}
                >
                  {isSleeping ? <span class="tab-sleep-dot" aria-label="Sleeping tab" /> : null}
                  {collapsed ? (
                    <span>{(tab.title || "N").slice(0, 1).toUpperCase()}</span>
                  ) : (
                    <span class="tab-title">{tab.title || tab.url || "New Tab"}</span>
                  )}
                </button>
                {!collapsed ? (
                  <button
                    type="button"
                    class="tab-close-btn"
                    aria-label={`Close ${tab.title || "tab"}`}
                    onClick={() => sendAction({ type: "CLOSE_TAB", tabId: tab.id })}
                  >
                    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
                      <path
                        d="M2 2l6 6M8 2l-6 6"
                        stroke="currentColor"
                        stroke-width="1.4"
                        stroke-linecap="round"
                      />
                    </svg>
                  </button>
                ) : null}
              </div>
            );
          })
        )}
      </div>

      <footer class="sidebar-footer">
        {runtimeUpdateVersion.value ? (
          <div class="runtime-pill" role="status">
            {collapsed
              ? "Upd"
              : `WebView2 ${runtimeUpdateVersion.value} ready (restart to apply)`}
          </div>
        ) : null}

        {!collapsed ? (
          <div class="sidebar-footer-row">
            <button type="button" class="secondary-btn" onClick={() => createTab(false)}>
              + Tab
            </button>
            <button type="button" class="secondary-btn" onClick={() => createTab(true)}>
              Private
            </button>
          </div>
        ) : null}

        <div class="sidebar-footer-row">
          <button
            type="button"
            class="icon-btn"
            aria-label="Open Command Palette"
            title="Command Palette (Ctrl+K)"
            onClick={() =>
              sendAction({ type: "EXECUTE_COMMAND", command: "open_command_palette" })
            }
          >
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.4" />
              <path
                d="M10.5 10.5L13.5 13.5"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
              />
            </svg>
          </button>

          {!collapsed ? (
            <>
              <button
                type="button"
                class="icon-btn"
                aria-label="Switch to Top Bar layout"
                title="Top Bar layout"
                onClick={toggleTopbar}
              >
                <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <rect
                    x="2"
                    y="2.5"
                    width="12"
                    height="11"
                    rx="2"
                    stroke="currentColor"
                    stroke-width="1.4"
                  />
                  <line x1="2" y1="6" x2="14" y2="6" stroke="currentColor" stroke-width="1.4" />
                </svg>
              </button>
              <button
                type="button"
                class="icon-btn"
                aria-label={`Switch to ${currentTheme === "dark" ? "light" : "dark"} theme`}
                title="Toggle theme"
                onClick={toggleTheme}
              >
                <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <circle cx="8" cy="8" r="3.5" stroke="currentColor" stroke-width="1.4" />
                  <path
                    d="M8 1.5V3M8 13v1.5M1.5 8H3M13 8h1.5"
                    stroke="currentColor"
                    stroke-width="1.4"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
              <button
                type="button"
                class="icon-btn"
                aria-label="Toggle Zen mode"
                title="Zen Mode (Ctrl+S)"
                onClick={() => sendAction({ type: "TOGGLE_ZEN" })}
              >
                <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <rect
                    x="2"
                    y="2.5"
                    width="12"
                    height="11"
                    rx="2"
                    stroke="currentColor"
                    stroke-width="1.4"
                  />
                </svg>
              </button>
            </>
          ) : null}
        </div>
      </footer>
    </aside>
  );
}
