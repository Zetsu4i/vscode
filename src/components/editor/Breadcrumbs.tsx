import { useWorkspaceStore } from "../../state/workspaceStore";
import { relativePath } from "../../util/paths";
import { useSettingsStore } from "../../state/settingsStore";
import { useUiStore } from "../../state/uiStore";
import FileIcon from "../shared/FileIcon";

/**
 * Path breadcrumbs above the editor: workspace-root-relative folder segments
 * ending in the file name, VSCode style. Clicking a folder segment reveals
 * it in the explorer.
 */
export default function Breadcrumbs({ path }: { path: string }) {
  const enabled = useSettingsStore((s) => s.breadcrumbs);
  const root = useWorkspaceStore((s) => s.root);
  const rootName = useWorkspaceStore((s) => s.rootName);
  const expanded = useWorkspaceStore((s) => s.expanded);
  const setView = useUiStore((s) => s.setView);

  if (!enabled) return null;

  const rel = root ? relativePath(root, path) : path;
  const segments = rel.split(/[\\/]/).filter(Boolean);

  const revealDir = (dirRel: string) => {
    // Expand every ancestor of the clicked segment, then show the explorer.
    if (root) {
      const store = useWorkspaceStore.getState();
      const sep = root.includes("\\") ? "\\" : "/";
      const parts = dirRel.split(/[\\/]/).filter(Boolean);
      let acc = root;
      for (const part of parts) {
        acc = acc + sep + part;
        if (!store.expanded[acc]) void store.toggleDir(acc);
      }
    }
    setView("explorer");
  };

  return (
    <div className="breadcrumbs" title={path}>
      {rootName && <span className="crumb crumb-root">{rootName}</span>}
      {segments.map((seg, i) => {
        const isFile = i === segments.length - 1;
        const dirRel = segments.slice(0, i + 1).join("/");
        const isExpandedDir = isFile ? false : !!expanded[dirRel];
        return (
          <span key={dirRel} className="crumb-wrap">
            <i className="codicon codicon-chevron-right crumb-sep" />
            <span
              className={`crumb ${isFile ? "crumb-file" : "crumb-dir"}`}
              onClick={() => {
                if (!isFile) revealDir(dirRel);
              }}
            >
              <FileIcon
                name={seg}
                isDir={!isFile}
                expanded={isExpandedDir}
                className="crumb-icon"
              />
              {seg}
            </span>
          </span>
        );
      })}
    </div>
  );
}
