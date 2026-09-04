import { useState } from "react";
import { useEditorStore, tabLabel, EditorGroup } from "../../state/editorStore";
import { languageForPath } from "../../util/paths";
import FileIcon from "../shared/FileIcon";
import { baseName } from "../../util/paths";

interface DragPayload {
  key: string;
  groupId: number;
}

export default function TabBar({ groupId }: { groupId: number }) {
  const group = useEditorStore((s) => s.groups.find((g) => g.id === groupId));
  const focusGroup = useEditorStore((s) => s.focusGroup);
  const setActive = useEditorStore((s) => s.setActive);
  const closeTab = useEditorStore((s) => s.closeTab);
  const reorderTab = useEditorStore((s) => s.reorderTab);
  const moveTabToGroup = useEditorStore((s) => s.moveTabToGroup);
  const buffers = useEditorStore((s) => s.buffers);
  const isActiveGroup = useEditorStore((s) => s.activeGroupId === groupId);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  const [isBarTarget, setIsBarTarget] = useState(false);

  if (!group || group.tabs.length === 0) return null;
  const g: EditorGroup = group;

  const readPayload = (e: React.DragEvent): DragPayload | null => {
    const raw = e.dataTransfer.getData("application/vstauri-tab");
    if (!raw) return null;
    try {
      return JSON.parse(raw) as DragPayload;
    } catch {
      return null;
    }
  };

  const handleDrop = (e: React.DragEvent, index: number | null) => {
    e.preventDefault();
    e.stopPropagation();
    setDropIndex(null);
    setIsBarTarget(false);
    const payload = readPayload(e);
    if (!payload) return;
    if (payload.groupId === groupId) {
      const from = g.tabs.findIndex((t) => t.key === payload.key);
      if (from < 0) return;
      const to = index === null ? g.tabs.length - 1 : index > from ? index - 1 : index;
      if (to !== from) reorderTab(groupId, from, to);
    } else {
      moveTabToGroup(payload.key, payload.groupId, groupId, index ?? undefined);
    }
  };

  return (
    <div
      className={`tabbar ${isActiveGroup ? "active-tabbar" : ""} ${isBarTarget ? "drop-target" : ""}`}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("application/vstauri-tab")) {
          e.preventDefault();
          setIsBarTarget(true);
        }
      }}
      onDragLeave={() => setIsBarTarget(false)}
      onDrop={(e) => handleDrop(e, null)}
    >
      {g.tabs.map((tab, i) => {
        const isActive = tab.key === g.activeKey;
        const buf = tab.kind === "file" ? buffers[tab.path] : null;
        const lang = languageForPath(tab.path);
        const iconClass =
          tab.kind === "diff"
            ? "codicon-diff-multiple"
            : tab.kind === "settings"
              ? "codicon-settings-gear"
              : tab.kind === "keybindings"
                ? "codicon-keyboard"
                : null;
        return (
          <div
            key={tab.key}
            className={`tab ${isActive ? "active" : ""} ${dropIndex === i ? "drop-before" : ""}`}
            onClick={() => {
              focusGroup(groupId);
              setActive(tab.key);
            }}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeTab(tab.key);
              }
            }}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(
                "application/vstauri-tab",
                JSON.stringify({ key: tab.key, groupId } satisfies DragPayload)
              );
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(e) => {
              if (e.dataTransfer.types.includes("application/vstauri-tab")) {
                e.preventDefault();
                e.stopPropagation();
                setDropIndex(i);
                setIsBarTarget(false);
              }
            }}
            onDrop={(e) => handleDrop(e, i)}
            title={tab.path}
          >
            {iconClass ? (
              <i className={`codicon ${iconClass} tab-icon`} />
            ) : (
              <FileIcon name={baseName(tab.path)} isDir={false} className="tab-icon" />
            )}
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
