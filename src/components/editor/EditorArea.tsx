import { useEditorStore } from "../../state/editorStore";
import TabBar from "./TabBar";
import MonacoPane from "./MonacoPane";
import DiffPane from "./DiffPane";
import ImagePane, { isImage } from "./ImagePane";
import Welcome from "./Welcome";

export default function EditorArea() {
  const tabs = useEditorStore((s) => s.tabs);
  const activeKey = useEditorStore((s) => s.activeKey);

  const active = tabs.find((t) => t.key === activeKey);

  return (
    <div className="editor-area">
      <TabBar />
      <div className="editor-body">
        {active?.kind === "file" && isImage(active.path) && (
          <ImagePane key={active.path} path={active.path} />
        )}
        {active?.kind === "file" && !isImage(active.path) && (
          <MonacoPane key={active.path} path={active.path} />
        )}
        {active?.kind === "diff" && <DiffPane key={active.path} path={active.path} />}
        {!active && <Welcome />}
      </div>
    </div>
  );
}
