import { Fragment, useCallback, useRef } from "react";
import { LayoutNode, useEditorStore } from "../../state/editorStore";
import TabBar from "./TabBar";
import MonacoPane from "./MonacoPane";
import DiffPane from "./DiffPane";
import Welcome from "./Welcome";
import Breadcrumbs from "./Breadcrumbs";
import HexView from "./HexView";
import SettingsEditor from "./SettingsEditor";
import KeybindingsEditor from "./KeybindingsEditor";

function Splitter({
  splitId,
  index,
  dir,
}: {
  splitId: number;
  index: number;
  dir: "row" | "column";
}) {
  const dragging = useRef(false);
  const start = useRef(0);
  const containerPx = useRef(1);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const parent = (e.target as HTMLElement).parentElement;
      containerPx.current =
        dir === "row" ? (parent?.clientWidth ?? 1) : (parent?.clientHeight ?? 1);
      dragging.current = true;
      start.current = dir === "row" ? e.clientX : e.clientY;
      const onMove = (ev: MouseEvent) => {
        if (!dragging.current) return;
        const cur = dir === "row" ? ev.clientX : ev.clientY;
        useEditorStore
          .getState()
          .resizeSplit(splitId, index, cur - start.current, containerPx.current);
        start.current = cur;
      };
      const onUp = () => {
        dragging.current = false;
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        document.body.classList.remove("split-resizing");
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
      document.body.classList.add("split-resizing");
    },
    [splitId, index, dir]
  );

  return (
    <div
      className={`splitter splitter-${dir}`}
      onMouseDown={onMouseDown}
      role="separator"
    />
  );
}

function NodeView({ node }: { node: LayoutNode }) {
  if (node.kind === "leaf") return <GroupPane groupId={node.groupId} />;
  return (
    <div className={`split-container split-${node.dir}`}>
      {node.children.map((child, i) => (
        <Fragment key={child.kind === "leaf" ? `g${child.groupId}` : `s${i}`}>
          {i > 0 && <Splitter splitId={node.id} index={i - 1} dir={node.dir} />}
          <div className="split-child" style={{ flexGrow: node.sizes[i], flexBasis: 0, minWidth: 0, minHeight: 0 }}>
            <NodeView node={child} />
          </div>
        </Fragment>
      ))}
    </div>
  );
}

function GroupPane({ groupId }: { groupId: number }) {
  const group = useEditorStore((s) => s.groups.find((g) => g.id === groupId));
  const focusGroup = useEditorStore((s) => s.focusGroup);
  const moveTabToGroup = useEditorStore((s) => s.moveTabToGroup);
  const isActiveGroup = useEditorStore((s) => s.activeGroupId === groupId);
  const dropDepth = useRef(0);

  if (!group) return null;

  const active = group.tabs.find((t) => t.key === group.activeKey);
  const activeBuf = active?.kind === "file" ? useEditorStore.getState().buffers[active.path] : null;

  const allowDrop = (e: React.DragEvent) => {
    if (e.dataTransfer.types.includes("application/vstauri-tab")) e.preventDefault();
  };
  const onDrop = (e: React.DragEvent) => {
    const raw = e.dataTransfer.getData("application/vstauri-tab");
    if (!raw) return;
    try {
      const { key, groupId: from } = JSON.parse(raw) as { key: string; groupId: number };
      moveTabToGroup(key, from, groupId);
    } catch {
      /* ignore malformed drops */
    }
  };

  return (
    <div
      className={`editor-group ${isActiveGroup ? "active-group" : ""}`}
      onMouseDown={() => focusGroup(groupId)}
      onDragOver={allowDrop}
      onDragEnter={(e) => {
        allowDrop(e);
        dropDepth.current++;
      }}
      onDragLeave={() => {
        dropDepth.current = Math.max(0, dropDepth.current - 1);
      }}
      onDrop={(e) => {
        onDrop(e);
        dropDepth.current = 0;
      }}
    >
      <TabBar groupId={groupId} />
      <div className="editor-body">
        {active?.kind === "file" && (
          <>
            <Breadcrumbs path={active.path} />
            {activeBuf?.truncated && !activeBuf?.binary && (
              <div className="truncated-banner">
                <i className="codicon codicon-warning" />
                Large file — showing the first 5 MB. Binary or oversized files
                open in the hex viewer.
              </div>
            )}
            <div className="editor-pane">
              <MonacoPane key={active.key} path={active.path} />
            </div>
          </>
        )}
        {active?.kind === "diff" && <DiffPane key={active.key} path={active.path} />}
        {active?.kind === "settings" && <SettingsEditor />}
        {active?.kind === "keybindings" && <KeybindingsEditor />}
        {!active && <Welcome />}
      </div>
    </div>
  );
}

export default function EditorArea() {
  const layout = useEditorStore((s) => s.layout);
  return (
    <div className="editor-area">
      <NodeView node={layout} />
    </div>
  );
}
