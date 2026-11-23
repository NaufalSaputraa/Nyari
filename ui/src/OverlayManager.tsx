import { effect } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";
import { CommandPalette } from "./CommandPalette";
import { ReaderView } from "./ReaderView";
import { Sidebar } from "./Sidebar";
import {
  activeOverlaySurface,
  activeTabId,
  commandPaletteOpen,
  findInPageOpen,
  findMatchInfo,
  findQuery,
  hoverSidebarOpen,
  modalQueue,
  popModal,
  readingArticle,
  readingModeOpen,
  sendAction,
  viewportSize
} from "./store";

function closeTopOverlay() {
  if (commandPaletteOpen.value) {
    commandPaletteOpen.value = false;
  } else if (findInPageOpen.value) {
    findInPageOpen.value = false;
    if (activeTabId.value) {
      sendAction({ type: "FIND_CLOSE", tabId: activeTabId.value });
    }
  } else if (readingModeOpen.value) {
    readingModeOpen.value = false;
  } else if (hoverSidebarOpen.value) {
    hoverSidebarOpen.value = false;
  }
  sendAction({ type: "CLOSE_OVERLAY" });
}

function FindInPageBar({ onClose }: { onClose: () => void }) {
  const inputRef = useRef<HTMLInputElement>(null);
  const tabId = activeTabId.value ?? "";

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  return (
    <section class="find-bar" aria-label="Find in page">
      <input
        ref={inputRef}
        type="text"
        class="find-input"
        value={findQuery.value}
        placeholder="Find in page..."
        aria-label="Find text in page"
        onInput={(e) => {
          const q = (e.currentTarget as HTMLInputElement).value;
          findQuery.value = q;
          if (tabId) {
            sendAction({
              type: "FIND_IN_PAGE",
              tabId,
              query: q,
              matchCase: false,
              wholeWord: false
            });
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && tabId) {
            sendAction({ type: "FIND_NEXT", tabId, forward: !e.shiftKey });
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          }
        }}
      />
      <span class="palette-item-meta">
        {findMatchInfo.value.activeMatch}/{findMatchInfo.value.matchCount}
      </span>
      <button
        type="button"
        class="icon-btn"
        aria-label="Previous match"
        disabled={!tabId}
        onClick={() => tabId && sendAction({ type: "FIND_NEXT", tabId, forward: false })}
      >
        &lt;
      </button>
      <button
        type="button"
        class="icon-btn"
        aria-label="Next match"
        disabled={!tabId}
        onClick={() => tabId && sendAction({ type: "FIND_NEXT", tabId, forward: true })}
      >
        &gt;
      </button>
      <button type="button" class="icon-btn" aria-label="Close find" onClick={onClose}>
        x
      </button>
    </section>
  );
}

export function OverlayManager() {
  const [promptInput, setPromptInput] = useState("");
  const [authUser, setAuthUser] = useState("");
  const [authPassword, setAuthPassword] = useState("");

  useEffect(() => {
    const dispose = effect(() => {
      const surface = activeOverlaySurface.value;
      const vp = viewportSize.value;

      if (!surface) {
        sendAction({
          type: "SET_OVERLAY_BOUNDS",
          visible: false,
          surface: null,
          xDip: 0,
          yDip: 0,
          widthDip: 0,
          heightDip: 0
        });
        return;
      }

      if (surface === "modal") {
        const w = Math.min(440, Math.max(300, vp.widthDip - 40));
        const h = 230;
        sendAction({
          type: "SET_OVERLAY_BOUNDS",
          visible: true,
          surface: "modal",
          xDip: Math.max(12, Math.round((vp.widthDip - w) / 2)),
          yDip: Math.max(48, Math.round((vp.heightDip - h) / 3)),
          widthDip: w,
          heightDip: h
        });
      } else if (surface === "command_palette") {
        if (findInPageOpen.value && !commandPaletteOpen.value) {
          const w = 340;
          const h = 44;
          sendAction({
            type: "SET_OVERLAY_BOUNDS",
            visible: true,
            surface: "command_palette",
            xDip: Math.max(12, vp.widthDip - w - 16),
            yDip: 44,
            widthDip: w,
            heightDip: h
          });
        } else {
          const w = Math.min(600, Math.max(300, vp.widthDip - 48));
          const h = Math.min(380, Math.max(240, vp.heightDip - 110));
          sendAction({
            type: "SET_OVERLAY_BOUNDS",
            visible: true,
            surface: "command_palette",
            xDip: Math.max(12, Math.round((vp.widthDip - w) / 2)),
            yDip: 72,
            widthDip: w,
            heightDip: h
          });
        }
      } else if (surface === "reading_mode") {
        const w = Math.min(820, Math.max(320, vp.widthDip - 80));
        const h = Math.max(300, vp.heightDip - 96);
        sendAction({
          type: "SET_OVERLAY_BOUNDS",
          visible: true,
          surface: "reading_mode",
          xDip: Math.max(16, Math.round((vp.widthDip - w) / 2)),
          yDip: 56,
          widthDip: w,
          heightDip: h
        });
      } else if (surface === "hover_sidebar") {
        sendAction({
          type: "SET_OVERLAY_BOUNDS",
          visible: true,
          surface: "hover_sidebar",
          xDip: 8,
          yDip: 40,
          widthDip: 230,
          heightDip: Math.max(280, vp.heightDip - 52)
        });
      }
    });

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && modalQueue.value.length === 0) {
        e.preventDefault();
        closeTopOverlay();
      }
    };
    window.addEventListener("keydown", onKeyDown);

    return () => {
      dispose();
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const currentModal = modalQueue.value[0];
  if (currentModal) {
    switch (currentModal.kind) {
      case "permission":
        return (
          <section class="overlay-root modal-card" role="dialog" aria-modal="true">
            <div>
              <h2 class="modal-title">Permission Request</h2>
              <p class="modal-desc">
                {currentModal.req.origin} requests access to {currentModal.req.kind}.
              </p>
            </div>
            <div class="modal-actions">
              <button
                type="button"
                class="secondary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_PERMISSION",
                    requestId: currentModal.req.requestId,
                    decision: "deny"
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Deny
              </button>
              <button
                type="button"
                class="primary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_PERMISSION",
                    requestId: currentModal.req.requestId,
                    decision: "allow"
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Allow
              </button>
            </div>
          </section>
        );
      case "script_dialog":
        return (
          <section class="overlay-root modal-card" role="dialog" aria-modal="true">
            <div>
              <h2 class="modal-title">Page Dialog ({currentModal.req.kind})</h2>
              <p class="modal-desc">{currentModal.req.message}</p>
              {currentModal.req.kind === "prompt" ? (
                <input
                  type="text"
                  class="find-input"
                  value={promptInput || currentModal.req.defaultText || ""}
                  onInput={(e) => setPromptInput((e.currentTarget as HTMLInputElement).value)}
                />
              ) : null}
            </div>
            <div class="modal-actions">
              {currentModal.req.kind !== "alert" ? (
                <button
                  type="button"
                  class="secondary-btn"
                  onClick={() => {
                    sendAction({
                      type: "RESOLVE_SCRIPT_DIALOG",
                      requestId: currentModal.req.requestId,
                      accepted: false
                    });
                    popModal(currentModal.req.requestId);
                    setPromptInput("");
                  }}
                >
                  Cancel
                </button>
              ) : null}
              <button
                type="button"
                class="primary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_SCRIPT_DIALOG",
                    requestId: currentModal.req.requestId,
                    accepted: true,
                    promptText:
                      currentModal.req.kind === "prompt"
                        ? promptInput || currentModal.req.defaultText
                        : undefined
                  });
                  popModal(currentModal.req.requestId);
                  setPromptInput("");
                }}
              >
                OK
              </button>
            </div>
          </section>
        );
      case "basic_auth":
        return (
          <section class="overlay-root modal-card" role="dialog" aria-modal="true">
            <div>
              <h2 class="modal-title">Authentication Required</h2>
              <p class="modal-desc">
                {currentModal.req.uri} ({currentModal.req.realm})
              </p>
              <div style={{ display: "flex", flexDirection: "column", gap: "6px", marginTop: "8px" }}>
                <input
                  type="text"
                  class="find-input"
                  placeholder="Username"
                  value={authUser}
                  onInput={(e) => setAuthUser((e.currentTarget as HTMLInputElement).value)}
                />
                <input
                  type="password"
                  class="find-input"
                  placeholder="Password"
                  value={authPassword}
                  onInput={(e) => setAuthPassword((e.currentTarget as HTMLInputElement).value)}
                />
              </div>
            </div>
            <div class="modal-actions">
              <button
                type="button"
                class="secondary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_BASIC_AUTH",
                    requestId: currentModal.req.requestId,
                    cancelled: true
                  });
                  popModal(currentModal.req.requestId);
                  setAuthUser("");
                  setAuthPassword("");
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                class="primary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_BASIC_AUTH",
                    requestId: currentModal.req.requestId,
                    cancelled: false,
                    username: authUser,
                    password: authPassword
                  });
                  popModal(currentModal.req.requestId);
                  setAuthUser("");
                  setAuthPassword("");
                }}
              >
                Sign In
              </button>
            </div>
          </section>
        );
      case "external_uri":
        return (
          <section class="overlay-root modal-card" role="dialog" aria-modal="true">
            <div>
              <h2 class="modal-title">Open External Application?</h2>
              <p class="modal-desc">{currentModal.req.uri}</p>
            </div>
            <div class="modal-actions">
              <button
                type="button"
                class="secondary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_EXTERNAL_URI",
                    requestId: currentModal.req.requestId,
                    allowed: false
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                class="primary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_EXTERNAL_URI",
                    requestId: currentModal.req.requestId,
                    allowed: true
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Open
              </button>
            </div>
          </section>
        );
      case "mcp_action":
        return (
          <section class="overlay-root modal-card" role="dialog" aria-modal="true">
            <div>
              <h2 class="modal-title">MCP Tool Approval: {currentModal.req.toolName}</h2>
              <p class="modal-desc">
                Origin: {currentModal.req.targetOrigin} ({currentModal.req.summary})
              </p>
            </div>
            <div class="modal-actions">
              <button
                type="button"
                class="secondary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_MCP_ACTION",
                    requestId: currentModal.req.requestId,
                    decision: "deny"
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Deny
              </button>
              <button
                type="button"
                class="secondary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_MCP_ACTION",
                    requestId: currentModal.req.requestId,
                    decision: "allow_once"
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Allow Once
              </button>
              <button
                type="button"
                class="primary-btn"
                onClick={() => {
                  sendAction({
                    type: "RESOLVE_MCP_ACTION",
                    requestId: currentModal.req.requestId,
                    decision: "allow_origin_session"
                  });
                  popModal(currentModal.req.requestId);
                }}
              >
                Allow Session
              </button>
            </div>
          </section>
        );
    }
  }

  if (commandPaletteOpen.value) {
    return <CommandPalette onClose={closeTopOverlay} />;
  }

  if (findInPageOpen.value) {
    return <FindInPageBar onClose={closeTopOverlay} />;
  }

  if (readingModeOpen.value) {
    return (
      <div class="overlay-root">
        <ReaderView article={readingArticle.value} onClose={closeTopOverlay} />
      </div>
    );
  }

  if (hoverSidebarOpen.value) {
    return <Sidebar collapsed={false} isHoverOverlay={true} onCloseOverlay={closeTopOverlay} />;
  }

  return null;
}
