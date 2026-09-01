import { useEditorStore, tabLabel } from "../../state/editorStore";
import { languageForPath } from "../../util/paths";

export default function TabBar() {
  const tabs = useEditorStore((s) => s.tabs);
  const activeKey = useEditorStore((s) => s.activeKey);
  const setActive = useEditorStore((s) => s.setActive);
  const closeTab = useEditorStore((s) => s.closeTab);
  const buffers = useEditorStore((s) => s.buffers);

  if (tabs.length === 0) return null;

  return (
    <div className="tabbar">
      {tabs.map((tab) => {
        const isActive = tab.key === activeKey;
        const buf = tab.kind === "file" ? buffers[tab.path] : null;
        const lang = languageForPath(tab.path);
        const iconClass =
          tab.kind === "diff"
            ? "codicon-diff-multiple"
            : lang === "json"
              ? "codicon-json"
              : "codicon-file-code";
        return (
          <div
            key={tab.key}
            className={`tab ${isActive ? "active" : ""}`}
            onClick={() => setActive(tab.key)}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeTab(tab.key);
              }
            }}
            title={tab.path}
          >
            <i className={`codicon ${iconClass} tab-icon`} />
            <span className="tab-label">{tabLabel(tab)}</span>
            {buf?.dirty && (
              <span className="tab-dirty" title="Unsaved changes" />
            )}
            <button
              className="tab-close"
              title="Close (Ctrl+W)"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.key);
              }}
            >
              <i className="codicon codicon-close" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
