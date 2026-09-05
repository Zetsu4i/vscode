import { pickAndOpenFolder } from "../../commands";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useUiStore } from "../../state/uiStore";

const SHORTCUTS: { keys: string; label: string; action?: () => void }[] = [
  {
    keys: "Ctrl+Shift+P",
    label: "Show All Commands",
    action: () => useUiStore.getState().openPalette("commands"),
  },
  {
    keys: "Ctrl+P",
    label: "Go to File",
    action: () => useUiStore.getState().openPalette("files"),
  },
  {
    keys: "Ctrl+`",
    label: "Toggle Terminal",
    action: () => useUiStore.getState().togglePanel(),
  },
  {
    keys: "Ctrl+B",
    label: "Toggle Side Bar",
    action: () => useUiStore.getState().toggleSidebar(),
  },
];

export default function Welcome() {
  const recents = useWorkspaceStore((s) => s.recentFolders);
  const openFolder = useWorkspaceStore((s) => s.openFolder);
  const root = useWorkspaceStore((s) => s.root);

  return (
    <div className="welcome">
      <div className="welcome-inner">
        <div className="welcome-cols">
          <div className="welcome-left">
            <h1 className="welcome-title">VSTauri</h1>
            <p className="welcome-sub">Editing evolved — rebuilt on Tauri 2 + Rust</p>

            <div className="welcome-section">
              <h2>Start</h2>
              <button className="welcome-link" onClick={() => void pickAndOpenFolder()}>
                <i className="codicon codicon-new-folder" /> Open Folder...
              </button>
            </div>

            {recents.length > 0 && (
              <div className="welcome-section">
                <h2>Recent</h2>
                {recents.map((r) => (
                  <button
                    key={r}
                    className="welcome-link"
                    disabled={r === root}
                    onClick={() => void openFolder(r)}
                  >
                    <i className="codicon codicon-folder" />
                    <span className="welcome-recent-name">{r.split(/[\\/]/).pop()}</span>
                    <span className="welcome-recent-path">{r}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="welcome-right">
            <div className="welcome-section">
              <h2>Keyboard Shortcuts</h2>
              {SHORTCUTS.map((s) => (
                <div key={s.keys} className="welcome-shortcut" onClick={s.action}>
                  <span className="welcome-shortcut-label">{s.label}</span>
                  <span className="welcome-keys">{s.keys}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
