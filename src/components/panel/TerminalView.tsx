import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { onPtyOutput, onPtyExit } from "../../ipc";
import { useTerminalStore, Term } from "../../state/terminalStore";
import { useUiStore } from "../../state/uiStore";
import { getActiveTheme, onThemeChange } from "../../theme";

function TerminalInstance({ term }: { term: Term }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const activeId = useTerminalStore((s) => s.activeId);
  const isActive = activeId === term.id;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const t = new Terminal({
      theme: getActiveTheme().xterm,
      fontSize: 14,
      fontFamily: 'Consolas, "Courier New", "Droid Sans Mono", monospace',
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host);
    try {
      fit.fit();
    } catch {
      /* zero-size host; refit on visible */
    }
    void import("../../ipc").then(({ ipc }) => {
      void ipc.resizePty(term.id, t.rows, t.cols);
    });

    const unlistenOut = onPtyOutput(term.id, (b64) => {
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      t.write(bytes);
    });

    const unlistenExit = onPtyExit(term.id, () => {
      t.write("\r\n\x1b[90m[Process exited]\x1b[0m\r\n");
      useTerminalStore.getState().removeLocal(term.id);
    });

    const dataSub = t.onData((d) => {
      void import("../../ipc").then(({ ipc }) => ipc.writePty(term.id, d));
    });
    const resizeSub = t.onResize(({ cols, rows }) => {
      void import("../../ipc").then(({ ipc }) => ipc.resizePty(term.id, rows, cols));
    });

    // Follow workbench theme changes live.
    const unTheme = onThemeChange((th) => {
      t.options.theme = th.xterm;
    });

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
    });
    ro.observe(host);

    return () => {
      unTheme();
      ro.disconnect();
      dataSub.dispose();
      resizeSub.dispose();
      void unlistenOut.then((f) => f());
      void unlistenExit.then((f) => f());
      t.dispose();
    };
  }, [term.id]);

  return (
    <div
      ref={hostRef}
      className="terminal-instance"
      style={{ display: isActive ? "block" : "none" }}
    />
  );
}

export default function TerminalView() {
  const terms = useTerminalStore((s) => s.terms);
  const activeId = useTerminalStore((s) => s.activeId);
  const setActive = useTerminalStore((s) => s.setActive);
  const create = useTerminalStore((s) => s.create);
  const kill = useTerminalStore((s) => s.kill);
  const root = useUiStore((s) => s.panelVisible);

  void root;

  return (
    <div className="terminal-view">
      <div className="terminal-instances">
        {terms.map((t) => (
          <TerminalInstance key={t.id} term={t} />
        ))}
        {terms.length === 0 && (
          <div className="terminal-empty">
            <p>There are no terminals open.</p>
            <button className="btn-secondary" onClick={() => void create()}>
              <i className="codicon codicon-add" /> New Terminal
            </button>
          </div>
        )}
      </div>
      {terms.length > 0 && (
        <div className="terminal-list">
          {terms.map((t) => (
            <div key={t.id} className={`terminal-list-item ${t.id === activeId ? "active" : ""}`}>
              <button className="terminal-list-name" onClick={() => setActive(t.id)}>
                {t.name}
              </button>
              <button
                className="terminal-list-kill"
                title="Kill Terminal"
                onClick={() => void kill(t.id)}
              >
                <i className="codicon codicon-trash" />
              </button>
            </div>
          ))}
          <button
            className="terminal-list-new"
            title="New Terminal (Ctrl+Shift+`)"
            onClick={() => void create()}
          >
            <i className="codicon codicon-add" />
          </button>
        </div>
      )}
    </div>
  );
}
