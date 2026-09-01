import { useCallback, useEffect, useRef } from "react";
import { useUiStore } from "../../state/uiStore";
import { useTerminalStore } from "../../state/terminalStore";
import TerminalView from "./TerminalView";
import ProblemsView from "./ProblemsView";

export default function BottomPanel() {
  const visible = useUiStore((s) => s.panelVisible);
  const height = useUiStore((s) => s.panelHeight);
  const setHeight = useUiStore((s) => s.setPanelHeight);
  const togglePanel = useUiStore((s) => s.togglePanel);
  const setPanelTab = useUiStore((s) => s.setPanelTab);
  const panelTab = useUiStore((s) => s.panelTab);
  const terms = useTerminalStore((s) => s.terms);
  const create = useTerminalStore((s) => s.create);
  const problems = useUiStore; // keep import used
  void problems;
  const dragging = useRef(false);

  const onMove = useCallback(
    (e: MouseEvent) => {
      if (dragging.current) setHeight(window.innerHeight - e.clientY - 22);
    },
    [setHeight]
  );
  const onUp = useCallback(() => {
    dragging.current = false;
    document.body.classList.remove("resizing-col");
  }, []);

  useEffect(() => {
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [onMove, onUp]);

  if (!visible) return null;

  return (
    <div className="bottom-panel" style={{ height }}>
      <div
        className="panel-resizer"
        onMouseDown={() => {
          dragging.current = true;
          document.body.classList.add("resizing-col");
        }}
      />
      <div className="panel-header">
        <div className="panel-tabs">
          <button
            className={`panel-tab ${panelTab === "problems" ? "active" : ""}`}
            onClick={() => setPanelTab("problems")}
          >
            Problems
          </button>
          <button
            className={`panel-tab ${panelTab === "terminal" ? "active" : ""}`}
            onClick={() => setPanelTab("terminal")}
          >
            Terminal
          </button>
        </div>
        <div className="panel-actions">
          {panelTab === "terminal" && (
            <>
              <button
                title="New Terminal (Ctrl+Shift+`)"
                onClick={() => void create()}
              >
                <i className="codicon codicon-add" />
              </button>
              <button
                title="Kill Terminal"
                onClick={() => {
                  const s = useTerminalStore.getState();
                  if (s.activeId) void s.kill(s.activeId);
                }}
              >
                <i className="codicon codicon-trash" />
              </button>
            </>
          )}
          <button title="Close Panel" onClick={togglePanel}>
            <i className="codicon codicon-close" />
          </button>
        </div>
      </div>
      <div className="panel-body">
        {panelTab === "terminal" ? (
          <TerminalView />
        ) : (
          <ProblemsView />
        )}
      </div>
      {panelTab === "terminal" && terms.length === 0 && null}
    </div>
  );
}
