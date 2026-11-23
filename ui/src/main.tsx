import { render } from "preact";
import "./theme.css";
import { OverlayManager } from "./OverlayManager";
import { Sidebar } from "./Sidebar";
import { TopBar, WindowControls } from "./TopBar";
import { activeTab, browserCrashReason, initIpcBridge, sendAction, settings } from "./store";

function ChromeLayer() {
  const cfg = settings.value;
  if (cfg.zen_mode) {
    return <div class="chrome-shell chrome-shell--hidden" />;
  }

  if (cfg.layout_mode === "topbar") {
    return (
      <div class="chrome-shell">
        <TopBar />
      </div>
    );
  }

  const collapsed = cfg.layout_mode === "vertical-collapsed";
  const current = activeTab.value;

  return (
    <div class="chrome-shell">
      <Sidebar collapsed={collapsed} />
      <header
        class={
          collapsed
            ? "top-caption-strip top-caption-strip--collapsed"
            : "top-caption-strip top-caption-strip--expanded"
        }
        aria-label="Window caption strip"
      >
        <div class="caption-actions">
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
            class="address-pill"
            aria-label="Open Command Palette"
            onClick={() =>
              sendAction({ type: "EXECUTE_COMMAND", command: "open_command_palette" })
            }
          >
            <span>{current?.url || "Search or enter URL (Ctrl+K)"}</span>
          </button>
          {browserCrashReason.value ? (
            <span class="runtime-pill" role="alert">
              Renderer interrupted: {browserCrashReason.value}
            </span>
          ) : null}
        </div>
        <div class="caption-drag-spacer" />
        <WindowControls />
      </header>
    </div>
  );
}

initIpcBridge();

const layer = new URLSearchParams(window.location.search).get("layer") ?? "chrome";
const root = document.getElementById("app");
if (root) {
  render(layer === "overlay" ? <OverlayManager /> : <ChromeLayer />, root);
}
