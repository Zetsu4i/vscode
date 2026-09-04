import { FileEntry } from "../../ipc";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useEditorStore, selectActiveKey } from "../../state/editorStore";
import { joinPath } from "../../util/paths";
import { MenuItem } from "../../state/uiStore";
import FileIcon from "../shared/FileIcon";

interface Props {
  entry: FileEntry;
  depth: number;
  onEntryMenu: (e: React.MouseEvent, path: string, isDir: boolean) => void;
}

export default function FileTree({ entry, depth, onEntryMenu }: Props) {
  const expanded = useWorkspaceStore((s) => s.expanded[entry.path] ?? false);
  const tree = useWorkspaceStore((s) => s.tree);
  const toggleDir = useWorkspaceStore((s) => s.toggleDir);
  const activeKey = useEditorStore(selectActiveKey);
  const openFile = useEditorStore((s) => s.openFile);
  const dirty = useEditorStore((s) => s.buffers[entry.path]?.dirty ?? false);

  const isActive = activeKey === entry.path;

  const handleClick = () => {
    if (entry.isDir) {
      void toggleDir(entry.path);
    } else {
      void openFile(entry.path);
    }
  };

  return (
    <div>
      <div
        className={`tree-row ${isActive ? "active" : ""}`}
        style={{ paddingLeft: 8 + depth * 8 }}
        onClick={handleClick}
        onContextMenu={(e) => onEntryMenu(e, entry.path, entry.isDir)}
        title={entry.path}
      >
        {entry.isDir ? (
          <i className={`codicon ${expanded ? "codicon-chevron-down" : "codicon-chevron-right"} tree-chevron`} />
        ) : (
          <span className="tree-chevron-spacer" />
        )}
        <FileIcon name={entry.name} isDir={entry.isDir} expanded={expanded} className="tree-icon" />
        <span className="tree-name">
          {entry.name}
          {dirty && <span className="tree-dirty-dot" />}
        </span>
      </div>
      {entry.isDir && expanded && tree[entry.path] && (
        <div>
          {tree[entry.path].map((child) => (
            <FileTree
              key={child.path}
              entry={child}
              depth={depth + 1}
              onEntryMenu={onEntryMenu}
            />
          ))}
        </div>
      )}
      {entry.isDir && expanded && !tree[entry.path] && (
        <div className="tree-loading" style={{ paddingLeft: 20 + depth * 8 }}>
          Loading...
        </div>
      )}
    </div>
  );
}
