import { useEffect, useMemo, useRef, useState } from "react";
import { useUiStore } from "../../state/uiStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useEditorStore } from "../../state/editorStore";
import { ipc } from "../../ipc";
import { commands, runCommand } from "../../commands";
import { baseName, relativePath } from "../../util/paths";
import { THEMES, applyTheme, getTheme } from "../../theme/themes";

/** Simple subsequence fuzzy score — higher is better, null = no match. */
function fuzzyScore(text: string, query: string): number | null {
  if (!query) return 0;
  const t = text.toLowerCase();
  const q = query.toLowerCase();
  let ti = 0;
  let score = 0;
  let streak = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    const idx = t.indexOf(ch, ti);
    if (idx === -1) return null;
    if (idx === ti && qi > 0) {
      streak += 1;
      score += 8 + streak; // consecutive bonus
    } else {
      streak = 0;
      score += 2;
      if (idx === 0 || "/\\-_ .".includes(t[idx - 1])) score += 6; // word start
    }
    score -= Math.min(4, idx - ti); // gap penalty
    ti = idx + 1;
  }
  return score - Math.min(10, text.length - q.length) * 0.05;
}

interface FileResult {
  path: string;
  score: number;
}

export default function QuickOpen() {
  const open = useUiStore((s) => s.paletteOpen);
  const mode = useUiStore((s) => s.paletteMode);
  const close = useUiStore((s) => s.closePalette);
  const themeId = useUiStore((s) => s.themeId);
  const setTheme = useUiStore((s) => s.setTheme);
  const [input, setInput] = useState("");
  const [files, setFiles] = useState<string[] | null>(null);
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const root = useWorkspaceStore((s) => s.root);

  useEffect(() => {
    if (open) {
      setInput(mode === "commands" ? ">" : "");
      setSelected(0);
      setFiles(null);
      setTimeout(() => inputRef.current?.focus(), 0);
      if (root) {
        ipc
          .listAllFiles(root)
          .then(setFiles)
          .catch(() => setFiles([]));
      }
    }
  }, [open, mode, root]);

  const isCommandMode = mode !== "themes" && (mode === "commands" || input.startsWith(">"));
  const isThemeMode = mode === "themes";
  const query = isCommandMode ? input.replace(/^>\s*/, "") : input;

  // Live theme preview: arrowing through the list applies the theme
  // immediately (without persisting) — exactly like VSCode's picker.
  const themeResults = useMemo(() => {
    if (!isThemeMode) return [];
    return THEMES.filter((t) =>
      query ? t.label.toLowerCase().includes(query.toLowerCase()) : true
    );
  }, [isThemeMode, query]);

  useEffect(() => {
    if (!isThemeMode || !open) return;
    const t = themeResults[selected];
    if (t) applyTheme(t.id, false);
  }, [isThemeMode, open, selected, themeResults]);

  const activeThemeId = isThemeMode ? getTheme(themeId).id : null;

  const fileResults = useMemo<FileResult[]>(() => {
    if (isCommandMode || !files) return [];
    return files
      .map((p) => {
        const rel = root ? relativePath(root, p) : p;
        const s1 = fuzzyScore(rel, query);
        const s2 = fuzzyScore(baseName(p), query);
        return { path: p, score: Math.max(s1 ?? -1, s2 ?? -1) };
      })
      .filter((r) => r.score > (query ? -1 : -Infinity))
      .sort((a, b) => b.score - a.score)
      .slice(0, 60);
  }, [files, query, isCommandMode, root]);

  const commandResults = useMemo(() => {
    if (!isCommandMode) return [];
    return commands
      .map((c) => ({
        cmd: c,
        score: fuzzyScore(`${c.category}: ${c.title}`, query),
      }))
      .filter((r) => r.score !== null)
      .sort((a, b) => (b.score ?? 0) - (a.score ?? 0))
      .slice(0, 60);
  }, [query, isCommandMode]);

  const resultCount = isCommandMode
    ? commandResults.length
    : isThemeMode
      ? themeResults.length
      : fileResults.length;

  useEffect(() => {
    setSelected(0);
  }, [input]);

  useEffect(() => {
    const el = listRef.current?.children[selected] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (!open) return null;

  const accept = (index: number) => {
    if (isThemeMode) {
      const t = themeResults[index];
      if (t) {
        setTheme(t.id);
        close();
      }
      return;
    }
    if (isCommandMode) {
      const item = commandResults[index];
      if (item) {
        close();
        void item.cmd.run();
      }
    } else {
      const item = fileResults[index];
      if (item) {
        close();
        void useEditorStore.getState().openFile(item.path);
      }
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((s) => Math.min(resultCount - 1, s + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((s) => Math.max(0, s - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      accept(selected);
    } else if (e.key === "Escape") {
      e.preventDefault();
      // Revert the live preview back to the committed theme.
      if (isThemeMode) applyTheme(getTheme(themeId).id, false);
      close();
    }
  };

  return (
    <div className="palette-overlay" onMouseDown={close}>
      <div
        className="palette"
        onMouseDown={(e) => e.stopPropagation()}
        style={{ top: 0 }}
      >
        <input
          ref={inputRef}
          className="palette-input"
          placeholder={
            isCommandMode
              ? "Type the name of a command to run"
              : isThemeMode
                ? "Select Color Theme (Up/Down Keys to Preview, Enter to Confirm)"
                : "Search files by name (use > for commands)"
          }
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          spellCheck={false}
        />
        <div className="palette-list" ref={listRef}>
          {isThemeMode
            ? themeResults.map((t, i) => (
                <div
                  key={t.id}
                  className={`palette-item ${i === selected ? "selected" : ""}`}
                  onMouseEnter={() => setSelected(i)}
                  onClick={() => accept(i)}
                >
                  <span className="palette-item-title">{t.label}</span>
                  <span className="palette-item-meta">
                    {t.id === activeThemeId ? "active" : t.kind}
                  </span>
                </div>
              ))
            : isCommandMode
            ? commandResults.map((r, i) => (
                <div
                  key={r.cmd.id}
                  className={`palette-item ${i === selected ? "selected" : ""}`}
                  onMouseEnter={() => setSelected(i)}
                  onClick={() => accept(i)}
                >
                  <span className="palette-item-title">{r.cmd.title}</span>
                  <span className="palette-item-meta">{r.cmd.category}</span>
                  {r.cmd.keybinding && (
                    <span className="palette-item-key">{r.cmd.keybinding}</span>
                  )}
                </div>
              ))
            : fileResults.map((r, i) => (
                <div
                  key={r.path}
                  className={`palette-item ${i === selected ? "selected" : ""}`}
                  onMouseEnter={() => setSelected(i)}
                  onClick={() => accept(i)}
                >
                  <i className="codicon codicon-file palette-item-icon" />
                  <span className="palette-item-title">{baseName(r.path)}</span>
                  <span className="palette-item-meta">
                    {root ? relativePath(root, r.path).replace(baseName(r.path), "") : r.path}
                  </span>
                </div>
              ))}
          {resultCount === 0 && (
            <div className="palette-empty">No matching results</div>
          )}
        </div>
      </div>
    </div>
  );
}
