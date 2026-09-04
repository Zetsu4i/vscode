import { useEffect, useMemo, useState } from "react";
import { commands } from "../../commands";
import {
  useKeybindingStore,
  resolveBindings,
  defaultRules,
  normalizeKey,
  displayKey,
} from "../../keybindings/store";

/**
 * Keyboard Shortcuts editor — lists every command with its effective
 * binding, lets the user assign a new key (press-to-capture) or reset
 * to the default. Changes persist to user keybindings.json.
 */
export default function KeybindingsEditor() {
  const userRules = useKeybindingStore((s) => s.userRules);
  const captureFor = useKeybindingStore((s) => s.captureFor);
  const setCapture = useKeybindingStore((s) => s.setCapture);
  const rebind = useKeybindingStore((s) => s.rebind);
  const [query, setQuery] = useState("");

  const resolved = useMemo(() => resolveBindings(userRules), [userRules]);
  const userKeys = useMemo(() => {
    const m = new Map<string, string[]>(); // commandId -> user-assigned keys
    for (const r of userRules) {
      if (r.command.startsWith("-")) continue;
      const k = normalizeKey(r.key);
      m.set(r.command, [...(m.get(r.command) ?? []), k]);
    }
    return m;
  }, [userRules]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return commands
      .map((c) => {
        const key = [...resolved.entries()].find(([, cmd]) => cmd === c.id)?.[0] ?? null;
        const isUser = userKeys.has(c.id);
        return { cmd: c, key, isUser };
      })
      .filter((r) => {
        if (!q) return true;
        return (
          r.cmd.title.toLowerCase().includes(q) ||
          r.cmd.id.toLowerCase().includes(q) ||
          (r.key ?? "").includes(q) ||
          r.cmd.category.toLowerCase().includes(q)
        );
      })
      .sort((a, b) => {
        if (!!b.key !== !!a.key) return a.key ? -1 : 1; // bound commands first
        return a.cmd.title.localeCompare(b.cmd.title);
      });
  }, [query, resolved, userKeys]);

  // While capturing, swallow everything (the global keydown handler resolves).
  useEffect(() => {
    if (!captureFor) return;
    const stop = (e: KeyboardEvent) => {
      if (e.key !== "Escape") e.preventDefault();
    };
    window.addEventListener("keyup", stop, true);
    return () => window.removeEventListener("keyup", stop, true);
  }, [captureFor]);

  return (
    <div className="keybindings-editor">
      <div className="settings-toolbar">
        <div className="settings-search">
          <i className="codicon codicon-search" />
          <input
            autoFocus
            type="text"
            placeholder="Type to search in keybindings"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <span className="keybindings-hint">
          Click a keybinding to change it — press the new combination, or Escape to cancel
        </span>
      </div>

      <div className="keybindings-list">
        <div className="keybindings-header">
          <span>Command</span>
          <span>Keybinding</span>
          <span>Source</span>
        </div>
        {rows.map(({ cmd, key, isUser }) => (
          <div key={cmd.id} className="keybinding-row">
            <div className="keybinding-cmd">
              <span className="keybinding-title">{cmd.title}</span>
              <span className="keybinding-id">{cmd.id}</span>
            </div>
            <div className="keybinding-key">
              {captureFor === cmd.id ? (
                <span className="kbd-capture">press desired keys…</span>
              ) : key ? (
                <button
                  className="kbd-chips"
                  title="Change keybinding"
                  onClick={() => setCapture(cmd.id)}
                >
                  {displayKey(key)}
                </button>
              ) : (
                <button className="kbd-empty" title="Assign keybinding" onClick={() => setCapture(cmd.id)}>
                  + add
                </button>
              )}
            </div>
            <div className="keybinding-source">
              {isUser ? "user" : key ? "default" : "—"}
              {isUser && (
                <button
                  className="keybinding-reset"
                  title="Reset to default"
                  onClick={() => void rebind(cmd.id, null)}
                >
                  <i className="codicon codicon-discard" />
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
      <div className="keybindings-footer">
        {defaultRules().length} default bindings · {userRules.length} user rule
        {userRules.length === 1 ? "" : "s"} · stored in keybindings.json
      </div>
    </div>
  );
}
