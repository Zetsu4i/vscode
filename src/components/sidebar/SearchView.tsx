import { useEffect, useMemo, useRef, useState } from "react";
import { useSearchStore } from "../../state/searchStore";
import { useEditorStore } from "../../state/editorStore";
import { useUiStore } from "../../state/uiStore";
import { SearchHit } from "../../ipc";
import { relativePath } from "../../util/paths";
import { useWorkspaceStore } from "../../state/workspaceStore";

function HitLine({ hit, onOpen }: { hit: SearchHit; onOpen: () => void }) {
  const text = hit.text.length > 500 ? hit.text.slice(0, 500) + "…" : hit.text;
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  hit.ranges.forEach(([s, e], i) => {
    if (s > cursor) parts.push(<span key={`t${i}`}>{text.slice(cursor, s)}</span>);
    parts.push(
      <span key={`m${i}`} className="search-match">
        {text.slice(s, e)}
      </span>
    );
    cursor = e;
  });
  if (cursor < text.length) parts.push(<span key="tail">{text.slice(cursor)}</span>);

  return (
    <div className="search-hit" style={{ paddingLeft: 28 }} onClick={onOpen}>
      <span className="search-hit-text">
        {parts}
      </span>
    </div>
  );
}

export default function SearchView() {
  const query = useSearchStore((s) => s.query);
  const setQuery = useSearchStore((s) => s.setQuery);
  const replace = useSearchStore((s) => s.replace);
  const setReplace = useSearchStore((s) => s.setReplace);
  const caseSensitive = useSearchStore((s) => s.caseSensitive);
  const wholeWord = useSearchStore((s) => s.wholeWord);
  const regex = useSearchStore((s) => s.regex);
  const toggleCase = useSearchStore((s) => s.toggleCase);
  const toggleWord = useSearchStore((s) => s.toggleWord);
  const toggleRegex = useSearchStore((s) => s.toggleRegex);
  const results = useSearchStore((s) => s.results);
  const searching = useSearchStore((s) => s.searching);
  const replacing = useSearchStore((s) => s.replacing);
  const filesScanned = useSearchStore((s) => s.filesScanned);
  const truncated = useSearchStore((s) => s.truncated);
  const run = useSearchStore((s) => s.run);
  const replaceAll = useSearchStore((s) => s.replaceAll);
  const root = useWorkspaceStore((s) => s.root);
  const openFile = useEditorStore((s) => s.openFile);
  const requestReveal = useUiStore((s) => s.requestReveal);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [replaceOpen, setReplaceOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (useUiStore.getState().view === "search") inputRef.current?.focus();
  }, []);

  const grouped = useMemo(() => {
    const m = new Map<string, SearchHit[]>();
    for (const hit of results) {
      const arr = m.get(hit.file);
      if (arr) arr.push(hit);
      else m.set(hit.file, [hit]);
    }
    return m;
  }, [results]);

  const openHit = (hit: SearchHit) => {
    void openFile(hit.file).then(() => {
      requestReveal(hit.file, hit.lineNumber, (hit.ranges[0]?.[0] ?? 0) + 1);
    });
  };

  const onInputKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void run();
    }
  };

  return (
    <div className="view">
      <div className="view-header">
        <span className="view-header-title">Search</span>
        <div className="view-header-actions">
          <button title="Refresh" onClick={() => void run()}>
            <i className="codicon codicon-refresh" />
          </button>
          <button title="Clear Search Results" onClick={() => useSearchStore.getState().setResults([])}>
            <i className="codicon codicon-clear-all" />
          </button>
        </div>
      </div>

      <div className="search-box">
        <div className="search-input-row">
          <button
            className={`toggle-btn ${replaceOpen ? "on" : ""}`}
            title={replaceOpen ? "Hide Replace" : "Toggle Replace"}
            onClick={() => setReplaceOpen((v) => !v)}
          >
            <i className={`codicon ${replaceOpen ? "codicon-chevron-down" : "codicon-chevron-right"}`} />
          </button>
          <input
            ref={inputRef}
            className="search-input"
            placeholder={root ? "Search" : "Open a folder to search"}
            value={query}
            disabled={!root}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onInputKeyDown}
            autoFocus
          />
          <button
            className={`toggle-btn ${caseSensitive ? "on" : ""}`}
            title="Match Case"
            onClick={toggleCase}
          >
            Aa
          </button>
          <button
            className={`toggle-btn ${wholeWord ? "on" : ""}`}
            title="Match Whole Word"
            onClick={toggleWord}
          >
            ab
          </button>
          <button
            className={`toggle-btn ${regex ? "on" : ""}`}
            title="Use Regular Expression"
            onClick={toggleRegex}
          >
            .*
          </button>
        </div>
        {replaceOpen && (
          <div className="search-input-row search-replace-row">
            <span className="search-replace-spacer" />
            <input
              className="search-input"
              placeholder="Replace"
              value={replace}
              disabled={!root}
              onChange={(e) => setReplace(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void replaceAll();
                }
              }}
            />
            <button
              className="toggle-btn"
              title={`Replace All${results.length ? ` (${results.length} results)` : ""}`}
              disabled={!root || results.length === 0 || replacing || searching}
              onClick={() => void replaceAll()}
            >
              <i className="codicon codicon-replace-all" />
            </button>
          </div>
        )}
      </div>

      {root ? (
        <div className="search-results">
          {searching && (
            <div className="search-status">Searching... {filesScanned} files scanned</div>
          )}
          {replacing && <div className="search-status">Replacing...</div>}
          {!searching && query && (
            <div className="search-status">
              {results.length === 0
                ? "No results found."
                : `${results.length} result${results.length === 1 ? "" : "s"} in ${grouped.size} file${grouped.size === 1 ? "" : "s"}`}
              {truncated ? " (results truncated)" : ""}
            </div>
          )}
          {[...grouped.entries()].map(([file, hits]) => {
            const isCollapsed = collapsed[file];
            return (
              <div key={file}>
                <div
                  className="search-file"
                  onClick={() => setCollapsed((c) => ({ ...c, [file]: !c[file] }))}
                >
                  <i
                    className={`codicon ${isCollapsed ? "codicon-chevron-right" : "codicon-chevron-down"} tree-chevron`}
                  />
                  <span className="search-file-name">{file.split(/[\\/]/).pop()}</span>
                  <span className="search-file-dir">{relativePath(root, file).replace(file.split(/[\\/]/).pop() ?? "", "")}</span>
                  <span className="search-file-count">{hits.length}</span>
                </div>
                {!isCollapsed &&
                  hits.slice(0, 50).map((hit, i) => (
                    <HitLine key={i} hit={hit} onOpen={() => openHit(hit)} />
                  ))}
              </div>
            );
          })}
        </div>
      ) : (
        <div className="view-empty">
          <p>Open a folder to search.</p>
        </div>
      )}
    </div>
  );
}
