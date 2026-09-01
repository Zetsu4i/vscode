import { useUiStore, SidebarView } from "../../state/uiStore";
import { useGitStore } from "../../state/gitStore";

const VIEWS: { id: SidebarView; icon: string; title: string }[] = [
  { id: "explorer", icon: "codicon-files", title: "Explorer (Ctrl+Shift+E)" },
  { id: "search", icon: "codicon-search", title: "Search (Ctrl+Shift+F)" },
  { id: "git", icon: "codicon-source-control", title: "Source Control (Ctrl+Shift+G)" },
  { id: "extensions", icon: "codicon-extensions", title: "Extensions" },
];

export default function ActivityBar() {
  const view = useUiStore((s) => s.view);
  const sidebarVisible = useUiStore((s) => s.sidebarVisible);
  const setView = useUiStore((s) => s.setView);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const changes = useGitStore((s) => s.changes);

  const gitCount = changes.length;

  const badgeFor = (v: SidebarView): string | null => {
    if (v === "git" && gitCount > 0) return String(gitCount);
    return null;
  };

  return (
    <div className="activitybar">
      <div className="activitybar-top">
        {VIEWS.map((v) => {
          const active = sidebarVisible && view === v.id;
          return (
            <button
              key={v.id}
              className={`activitybar-item ${active ? "active" : ""}`}
              title={v.title}
              onClick={() => {
                if (view === v.id && sidebarVisible) {
                  toggleSidebar();
                } else {
                  setView(v.id);
                }
              }}
            >
              <i className={`codicon ${v.icon}`} />
              {badgeFor(v.id) && <span className="activitybar-badge">{badgeFor(v.id)}</span>}
            </button>
          );
        })}
      </div>
      <div className="activitybar-bottom">
        <button
          className="activitybar-item"
          title="Accounts"
        >
          <i className="codicon codicon-account" />
        </button>
        <button
          className="activitybar-item"
          title="Manage"
          onClick={() => useUiStore.getState().openPalette("commands")}
        >
          <i className="codicon codicon-settings-gear" />
        </button>
      </div>
    </div>
  );
}
