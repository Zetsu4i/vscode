import { useUiStore } from "../../state/uiStore";
import { useGitStore } from "../../state/gitStore";
import { useEditorStore, selectActiveKey } from "../../state/editorStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { languageForPath, baseName } from "../../util/paths";

export default function StatusBar() {
  const cursor = useUiStore((s) => s.cursor);
  const setPanelTab = useUiStore((s) => s.setPanelTab);
  const setView = useUiStore((s) => s.setView);
  const branch = useGitStore((s) => s.branch);
  const repo = useGitStore((s) => s.repo);
  const changes = useGitStore((s) => s.changes);
  const refreshGit = useGitStore((s) => s.refresh);
  const activeKey = useEditorStore(selectActiveKey);
  const buffers = useEditorStore((s) => s.buffers);
  const problems = useEditorStore((s) => s.problems);
  const root = useWorkspaceStore((s) => s.root);

  const errors = problems.filter((p) => p.severity === 1).length;
  const warnings = problems.filter((p) => p.severity === 2).length;
  const activePath = activeKey && !activeKey.startsWith("diff:") ? activeKey : null;
  const lang = activePath ? languageForPath(activePath) : "";
  const langLabel = lang ? lang.charAt(0).toUpperCase() + lang.slice(1) : "Plain Text";
  const buf = activePath ? buffers[activePath] : null;
  const remoteCount = changes.filter((c) => c.x !== " " && c.x !== "?").length;
  const localCount = changes.filter(
    (c) => (c.y !== " " && c.y !== "?") || c.x === "?"
  ).length;

  return (
    <div className="statusbar">
      <div className="statusbar-left">
        {repo && branch && (
          <>
            <button
              className="statusbar-item"
              title="Checkout branch"
              onClick={() => setView("git")}
            >
              <i className="codicon codicon-git-branch" />
              <span>{branch}</span>
            </button>
            <button
              className="statusbar-item"
              title="Refresh source control"
              onClick={() => void refreshGit()}
            >
              <i className="codicon codicon-sync" />
              {remoteCount + localCount > 0 && (
                <span>
                  {remoteCount}↓ {localCount}↑
                </span>
              )}
            </button>
          </>
        )}
        <button
          className="statusbar-item"
          title="Problems (Ctrl+Shift+M)"
          onClick={() => setPanelTab("problems")}
        >
          <i className="codicon codicon-error" />
          <span>{errors}</span>
          <i className="codicon codicon-warning" style={{ marginLeft: 6 }} />
          <span>{warnings}</span>
        </button>
      </div>
      <div className="statusbar-right">
        {activePath && (
          <>
            <button className="statusbar-item" title="Go to Line/Column">
              <span>
                Ln {cursor.line}, Col {cursor.col}
              </span>
            </button>
            <button className="statusbar-item" title="Select Indentation">
              <span>Spaces: 4</span>
            </button>
            <button className="statusbar-item" title="Select Encoding">
              <span>UTF-8</span>
            </button>
            <button className="statusbar-item" title="Select End of Line Sequence">
              <span>LF</span>
            </button>
            <button
              className="statusbar-item"
              title="Select Language Mode"
              onClick={() => setView("extensions")}
            >
              <span>{langLabel}</span>
            </button>
            {buf?.dirty && (
              <span className="statusbar-item" title="Unsaved changes">
                <i className="codicon codicon-circle-filled" style={{ fontSize: 9 }} />
              </span>
            )}
          </>
        )}
        {root && !activePath && (
          <button
            className="statusbar-item"
            title="Open folder"
            onClick={() => setView("explorer")}
          >
            <i className="codicon codicon-folder" />
            <span>{baseName(root)}</span>
          </button>
        )}
      </div>
    </div>
  );
}
