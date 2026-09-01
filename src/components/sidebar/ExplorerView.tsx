import { joinPath, baseName, dirName } from "../../util/paths";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useEditorStore } from "../../state/editorStore";
import { useUiStore, MenuItem } from "../../state/uiStore";
import { ipc } from "../../ipc";
import { pickAndOpenFolder } from "../../commands";
import FileTree from "./FileTree";

export default function ExplorerView() {
  const root = useWorkspaceStore((s) => s.root);
  const rootName = useWorkspaceStore((s) => s.rootName);
  const tree = useWorkspaceStore((s) => s.tree);
  const toggleDir = useWorkspaceStore((s) => s.toggleDir);
  const refreshAll = useWorkspaceStore((s) => s.refreshAll);
  const openFile = useEditorStore((s) => s.openFile);
  const openContextMenu = useUiStore((s) => s.openContextMenu);
  const showInput = useUiStore((s) => s.showInput);
  const showConfirm = useUiStore((s) => s.showConfirm);

  if (!root) {
    return (
      <div className="view-empty">
        <p>You have not yet opened a folder.</p>
        <button className="btn-primary" onClick={() => void pickAndOpenFolder()}>
          Open Folder
        </button>
      </div>
    );
  }

  const entries = tree[root];

  const newFileAt = (dir: string) => {
    showInput({
      title: "New File",
      value: "",
      placeholder: "name.ts",
      onOk: async (name) => {
        if (!name.trim()) return;
        try {
          await ipc.createFile(joinPath(dir, name.trim()));
          await refreshAll();
          void openFile(joinPath(dir, name.trim()));
        } catch (e) {
          console.error(e);
        }
      },
    });
  };

  const newFolderAt = (dir: string) => {
    showInput({
      title: "New Folder",
      value: "",
      placeholder: "folder name",
      onOk: async (name) => {
        if (!name.trim()) return;
        try {
          await ipc.createDir(joinPath(dir, name.trim()));
          await refreshAll();
        } catch (e) {
          console.error(e);
        }
      },
    });
  };

  const rootMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const items: MenuItem[] = [
      { label: "New File...", icon: "codicon-new-file", action: () => newFileAt(root) },
      { label: "New Folder...", icon: "codicon-new-folder", action: () => newFolderAt(root) },
      { separator: true },
      { label: "Refresh Explorer", icon: "codicon-refresh", action: () => void refreshAll() },
    ];
    openContextMenu(e.clientX, e.clientY, items);
  };

  const entryMenu = (e: React.MouseEvent, path: string, isDir: boolean) => {
    e.preventDefault();
    e.stopPropagation();
    const parent = dirName(path);
    const items: MenuItem[] = [];
    if (isDir) {
      items.push(
        { label: "New File...", icon: "codicon-new-file", action: () => newFileAt(path) },
        { label: "New Folder...", icon: "codicon-new-folder", action: () => newFolderAt(path) },
        { separator: true }
      );
    } else {
      items.push({
        label: "Open",
        icon: "codicon-go-to-file",
        action: () => void openFile(path),
      });
    }
    items.push(
      {
        label: "Rename...",
        icon: "codicon-edit",
        action: () => {
          showInput({
            title: "Rename",
            value: baseName(path),
            onOk: async (name) => {
              if (!name.trim() || name === baseName(path)) return;
              const target = joinPath(parent, name.trim());
              try {
                await ipc.renamePath(path, target);
                useEditorStore.getState().handleRename(path, target);
                await refreshAll();
              } catch (err) {
                console.error(err);
              }
            },
          });
        },
      },
      {
        label: "Delete",
        icon: "codicon-trash",
        danger: true,
        action: () => {
          showConfirm({
            title: `Delete ${baseName(path)}?`,
            message: "This action cannot be undone.",
            okLabel: "Delete",
            onOk: async () => {
              try {
                await ipc.deletePath(path, true);
                useEditorStore.getState().handleDelete(path);
                await refreshAll();
              } catch (err) {
                console.error(err);
              }
            },
          });
        },
      },
      { separator: true },
      {
        label: "Copy Path",
        icon: "codicon-copy",
        action: () => void navigator.clipboard.writeText(path),
      }
    );
    openContextMenu(e.clientX, e.clientY, items);
  };

  return (
    <div className="view">
      <div className="view-header">
        <span className="view-header-title">Explorer</span>
        <div className="view-header-actions">
          <button title="New File..." onClick={() => newFileAt(root)}>
            <i className="codicon codicon-new-file" />
          </button>
          <button title="New Folder..." onClick={() => newFolderAt(root)}>
            <i className="codicon codicon-new-folder" />
          </button>
          <button title="Refresh Explorer" onClick={() => void refreshAll()}>
            <i className="codicon codicon-refresh" />
          </button>
          <button title="Collapse Folders" onClick={() => useWorkspaceStore.setState({ expanded: {} })}>
            <i className="codicon codicon-collapse-all" />
          </button>
        </div>
      </div>
      <div className="view-section-header">
        <i className="codicon codicon-chevron-down" />
        <span className="view-section-name" onContextMenu={rootMenu}>
          {rootName.toUpperCase()}
        </span>
      </div>
      <div className="file-tree" onContextMenu={rootMenu}>
        {entries === undefined ? (
          <div className="tree-loading">Loading...</div>
        ) : entries.length === 0 ? (
          <div className="tree-empty">Empty folder</div>
        ) : (
          entries.map((entry) => (
            <FileTree
              key={entry.path}
              entry={entry}
              depth={0}
              onEntryMenu={entryMenu}
            />
          ))
        )}
      </div>
    </div>
  );
}
