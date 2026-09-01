import { useState } from "react";
import { useGitStore } from "../../state/gitStore";
import { useEditorStore } from "../../state/editorStore";
import { useUiStore } from "../../state/uiStore";
import { GitChange } from "../../ipc";
import { baseName } from "../../util/paths";

const STATUS_COLORS: Record<string, string> = {
  M: "var(--git-modified)",
  A: "var(--git-added)",
  D: "var(--git-deleted)",
  U: "var(--git-untracked)",
  "?": "var(--git-untracked)",
  R: "var(--git-renamed)",
  C: "var(--git-renamed)",
};

function changeLetter(c: GitChange): string {
  if (c.x === "?" && c.y === "?") return "U";
  if (c.x !== " ") return c.x;
  return c.y;
}

export default function GitView() {
  const repo = useGitStore((s) => s.repo);
  const changes = useGitStore((s) => s.changes);
  const branch = useGitStore((s) => s.branch);
  const stage = useGitStore((s) => s.stage);
  const unstage = useGitStore((s) => s.unstage);
  const commit = useGitStore((s) => s.commit);
  const refresh = useGitStore((s) => s.refresh);
  const openDiff = useEditorStore((s) => s.openDiff);
  const root = useGitStore; // placeholder to satisfy lint on unused hooks pattern
  const [message, setMessage] = useState("");
  const [committing, setCommitting] = useState(false);
  const openContextMenu = useUiStore((s) => s.openContextMenu);

  void root;

  const staged = changes.filter((c) => c.x !== " " && c.x !== "?");
  const unstaged = changes.filter((c) => (c.y !== " " && c.y !== "?") || c.x === "?");

  const rowMenu = (e: React.MouseEvent, c: GitChange) => {
    e.preventDefault();
    e.stopPropagation();
    openContextMenu(e.clientX, e.clientY, [
      { label: "Open Diff", icon: "codicon-diff-multiple", action: () => void openDiff(c.path) },
      { separator: true },
      {
        label: "Stage",
        icon: "codicon-add",
        action: () => void stage([c.path]),
      },
      {
        label: "Unstage",
        icon: "codicon-discard",
        action: () => void unstage([c.path]),
      },
    ]);
  };

  const doCommit = async () => {
    if (!message.trim() || committing) return;
    setCommitting(true);
    try {
      await commit(message.trim());
      setMessage("");
    } catch (e) {
      console.error(e);
    } finally {
      setCommitting(false);
    }
  };

  if (!repo) {
    return (
      <div className="view">
        <div className="view-header">
          <span className="view-header-title">Source Control</span>
        </div>
        <div className="view-empty">
          <p>The folder currently open doesn't have a Git repository.</p>
          <button className="btn-secondary" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>
      </div>
    );
  }

  const renderGroup = (title: string, items: GitChange[], isStaged: boolean) =>
    items.length > 0 && (
      <>
        <div className="view-section-header">
          <i className="codicon codicon-chevron-down" />
          <span className="view-section-name">
            {title} ({items.length})
          </span>
          {!isStaged && items.length > 0 && (
            <button
              className="section-action"
              title="Stage All Changes"
              onClick={() => void stage(items.map((c) => c.path))}
            >
              <i className="codicon codicon-add" />
            </button>
          )}
        </div>
        {items.map((c) => {
          const letter = changeLetter(c);
          const color = STATUS_COLORS[letter] ?? "var(--text-muted)";
          return (
            <div
              key={c.path}
              className="tree-row git-row"
              onClick={() => void openDiff(c.path)}
              onContextMenu={(e) => rowMenu(e, c)}
              title={c.path}
            >
              <span className="git-letter" style={{ color }}>
                {letter}
              </span>
              <span className="tree-name">{baseName(c.path)}</span>
              <span className="git-path">{c.origPath ? `${c.origPath} → ` : ""}</span>
              <span className="git-actions">
                {isStaged ? (
                  <button
                    title="Unstage Changes"
                    onClick={(e) => {
                      e.stopPropagation();
                      void unstage([c.path]);
                    }}
                  >
                    <i className="codicon codicon-remove" />
                  </button>
                ) : (
                  <button
                    title="Stage Changes"
                    onClick={(e) => {
                      e.stopPropagation();
                      void stage([c.path]);
                    }}
                  >
                    <i className="codicon codicon-add" />
                  </button>
                )}
              </span>
            </div>
          );
        })}
      </>
    );

  return (
    <div className="view">
      <div className="view-header">
        <span className="view-header-title">Source Control</span>
        <div className="view-header-actions">
          <button title="Refresh" onClick={() => void refresh()}>
            <i className="codicon codicon-refresh" />
          </button>
        </div>
      </div>

      <div className="commit-box">
        <textarea
          className="commit-message"
          placeholder={`Message (Ctrl+Enter to commit on "${branch ?? "branch"}")`}
          value={message}
          rows={3}
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
              e.preventDefault();
              void doCommit();
            }
          }}
        />
        <button
          className="btn-primary commit-btn"
          disabled={!message.trim() || committing}
          onClick={() => void doCommit()}
        >
          <i className="codicon codicon-check" />
          {committing ? "Committing..." : "Commit"}
        </button>
      </div>

      <div className="git-changes">
        {renderGroup("Staged Changes", staged, true)}
        {renderGroup("Changes", unstaged, false)}
      </div>
    </div>
  );
}
