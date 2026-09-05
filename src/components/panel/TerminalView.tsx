import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { onPtyOutput, onPtyExit, ipc, ShellInfo } from "../../ipc";
import { useTerminalStore, Term } from "../../state/terminalStore";
import { useUiStore, MenuItem } from "../../state/uiStore";
import { getTheme } from "../../theme/themes";

function TerminalInstance({ term }: { term: Term }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const activeId = useTerminalStore((s) => s.activeId);
  const isActive = activeId === term.id;
  const themeId = useUiStore((s) => s.themeId);
  const termRef = useRef<Terminal | null>(null);

  // Live theme switching — mirrors VSCode's terminal.integrated integration.
  useEffect(() => {
    const t = termRef.current;
    if (t) t.options.theme = getTheme(themeId).xterm;
  }, [themeId]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const t = new Terminal({
      theme: getTheme(themeId).xterm,
      fontSize: 14,
      fontFamily: 'Consolas, "Courier New", "Droid Sans Mono", monospace',
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host);
    termRef.current = t;
    try {
      fit.fit();
    } catch {
      /* zero-size host; refit on visible */
    }
    void import("../../ipc").then(({ ipc }) => {
      void ipc.resizePty(term.id, t.rows, t.cols);
    });

    let pendingAck = 0;
    let ackTimer: ReturnType<typeof setTimeout> | null = null;
    const flushAck = () => {
      ackTimer = null;
      if (pendingAck > 0) {
        void ipc.ackPty(term.id, pendingAck).catch(() => {});
        pendingAck = 0;
      }
    };

    const unlistenOut = onPtyOutput(term.id, (b64) => {
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      t.write(bytes);
      // Flow control: batch-ack consumed bytes so the backend pump resumes
      // before the high watermark without drowning IPC in tiny calls.
      pendingAck += bytes.length;
      if (pendingAck >= 64 * 1024) {
        if (ackTimer) clearTimeout(ackTimer);
        flushAck();
      } else if (ackTimer === null) {
        ackTimer = setTimeout(flushAck, 32);
      }
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

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
    });
    ro.observe(host);

    // Attach now that the output listener is live: everything the shell
    // printed before this point (prompt, MOTD) is flushed from the Rust
    // pre-attach buffer, in order. Previously this early output was lost.
    void ipc.attachPty(term.id).catch(() => {});

    return () => {
      ro.disconnect();
      termRef.current = null;
      dataSub.dispose();
      resizeSub.dispose();
      if (ackTimer) clearTimeout(ackTimer);
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
  const shellsRef = useRef<ShellInfo[] | null>(null);
  void root;

  // Shell profile picker (the dropdown next to "+") — shells are enumerated
  // once by the backend on first open.
  const openShellMenu = async (x: number, y: number) => {
    try {
      if (!shellsRef.current) {
        shellsRef.current = await ipc.listShells();
      }
      const items: MenuItem[] = shellsRef.current.map((s) => ({
        label: `${s.name}${s.default ? "   (default)" : ""}`,
        action: () => void create(undefined, s.path),
      }));
      if (items.length) useUiStore.getState().openContextMenu(x, y, items);
    } catch {
      /* enumeration unavailable — the "+" button still works */
    }
  };

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
          <button
            className="terminal-list-new"
            title="Select Terminal Profile"
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              void openShellMenu(r.left, r.bottom + 4);
            }}
          >
            <i className="codicon codicon-chevron-down" />
          </button>
        </div>
      )}
    </div>
  );
}
