import { useEffect, useRef, useState } from "preact/hooks";
import {
  activeTab,
  activeWorkspaceId,
  omniboxResults,
  sendAction,
  settings,
  switchTab,
  tabs
} from "./store";

interface CommandPaletteProps {
  onClose: () => void;
}

interface PaletteEntry {
  id: string;
  title: string;
  meta: string;
  run: () => void;
}

export function CommandPalette({ onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    sendAction({ type: "QUERY_OMNIBOX", query: "" });
  }, []);

  const currentTab = activeTab.value;
  const builtinCommands: PaletteEntry[] = [
    {
      id: "cmd-new-tab",
      title: "New Tab",
      meta: "Ctrl+T",
      run: () => sendAction({ type: "CREATE_TAB", workspaceId: activeWorkspaceId.value })
    },
    {
      id: "cmd-private-tab",
      title: "New Private Tab",
      meta: "Ctrl+Shift+N",
      run: () =>
        sendAction({
          type: "CREATE_TAB",
          workspaceId: activeWorkspaceId.value,
          isPrivate: true
        })
    },
    {
      id: "cmd-toggle-zen",
      title: settings.value.zen_mode ? "Exit Zen Mode" : "Enter Zen Mode",
      meta: "Ctrl+S",
      run: () => sendAction({ type: "TOGGLE_ZEN" })
    },
    {
      id: "cmd-toggle-theme",
      title: `Switch to ${settings.value.theme === "dark" ? "Light" : "Dark"} Theme`,
      meta: "/theme",
      run: () =>
        sendAction({
          type: "UPDATE_SETTINGS",
          patch: { theme: settings.value.theme === "dark" ? "light" : "dark" }
        })
    },
    {
      id: "cmd-open-in-edge",
      title: "Open Current Page in Microsoft Edge (Passkey Escape Hatch)",
      meta: "/open-in-edge",
      run: () => {
        if (currentTab?.url) {
          sendAction({ type: "OPEN_IN_EDGE", url: currentTab.url });
        }
      }
    }
  ];

  const normalized = query.trim().toLowerCase();
  const localTabs: PaletteEntry[] = tabs.value
    .filter(
      (t) =>
        !normalized ||
        t.title.toLowerCase().includes(normalized) ||
        t.url.toLowerCase().includes(normalized)
    )
    .map((t) => ({
      id: `tab-${t.id}`,
      title: t.title || t.url || "Untitled Tab",
      meta: "tab",
      run: () => switchTab(t.id)
    }));

  const remoteHits: PaletteEntry[] = omniboxResults.value.map((item) => ({
    id: `omni-${item.id}`,
    title: item.title || item.url,
    meta: item.source,
    run: () => {
      if (item.source === "command") {
        sendAction({ type: "EXECUTE_COMMAND", command: item.url });
      } else if (currentTab) {
        sendAction({ type: "NAVIGATE", tabId: currentTab.id, urlOrQuery: item.url });
      } else {
        sendAction({
          type: "CREATE_TAB",
          workspaceId: activeWorkspaceId.value,
          url: item.url
        });
      }
    }
  }));

  const filteredCommands = builtinCommands.filter(
    (cmd) =>
      !normalized ||
      cmd.title.toLowerCase().includes(normalized) ||
      cmd.meta.toLowerCase().includes(normalized)
  );

  const entries = [...filteredCommands, ...localTabs, ...remoteHits];

  const executeQueryOrSelection = () => {
    const trimmed = query.trim();
    if (trimmed === "/open-in-edge" && currentTab?.url) {
      sendAction({ type: "OPEN_IN_EDGE", url: currentTab.url });
      onClose();
      return;
    }

    const picked = entries[selectedIndex];
    if (picked && (trimmed.startsWith("/") || trimmed.length === 0)) {
      picked.run();
      onClose();
      return;
    }

    if (trimmed.length > 0) {
      if (currentTab) {
        sendAction({ type: "NAVIGATE", tabId: currentTab.id, urlOrQuery: trimmed });
      } else {
        sendAction({
          type: "CREATE_TAB",
          workspaceId: activeWorkspaceId.value,
          url: trimmed
        });
      }
      onClose();
    } else if (picked) {
      picked.run();
      onClose();
    }
  };

  return (
    <section class="overlay-root palette-box" aria-label="Command Palette">
      <div class="palette-input-row">
        <input
          ref={inputRef}
          type="text"
          class="palette-input"
          value={query}
          aria-label="Search tabs, history, bookmarks, or commands"
          placeholder="Search tabs, history, bookmarks, URL, or /command..."
          onInput={(e) => {
            const next = (e.currentTarget as HTMLInputElement).value;
            setQuery(next);
            setSelectedIndex(0);
            sendAction({ type: "QUERY_OMNIBOX", query: next });
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelectedIndex((idx) => Math.min(idx + 1, Math.max(0, entries.length - 1)));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelectedIndex((idx) => Math.max(0, idx - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              executeQueryOrSelection();
            } else if (e.key === "Escape") {
              e.preventDefault();
              onClose();
            }
          }}
        />
        <button type="button" class="secondary-btn" onClick={onClose}>
          Esc
        </button>
      </div>

      <div class="palette-results" role="listbox" aria-label="Results">
        {entries.length === 0 ? (
          <div class="palette-item">
            <span>Press Enter to search or navigate to "{query.trim()}"</span>
            <span class="palette-item-meta">Enter</span>
          </div>
        ) : (
          entries.map((item, idx) => (
            <button
              key={item.id}
              type="button"
              role="option"
              aria-selected={idx === selectedIndex}
              class={
                idx === selectedIndex ? "palette-item palette-item--selected" : "palette-item"
              }
              onClick={() => {
                item.run();
                onClose();
              }}
            >
              <span>{item.title}</span>
              <span class="palette-item-meta">{item.meta}</span>
            </button>
          ))
        )}
      </div>
    </section>
  );
}
