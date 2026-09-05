import { useEditorStore } from "../../state/editorStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { relativePath } from "../../util/paths";
import TabBar from "./TabBar";
import MonacoPane from "./MonacoPane";
import DiffPane from "./DiffPane";
import ImagePane, { isImage } from "./ImagePane";
import SettingsPane from "../settings/SettingsPane";
import Welcome from "./Welcome";

function Breadcrumbs({ path }: { path: string }) {
  const root = useWorkspaceStore((s) => s.root);
  const rootName = useWorkspaceStore((s) => s.rootName);
  if (!root) return null;
  const segs = relativePath(root, path).split(/[\\/]/).filter(Boolean);
  return (
    <div className="breadcrumbs">
      <span className="breadcrumbs-seg">
        <i className="codicon codicon-folder" />
        {rootName}
      </span>
      {segs.map((seg, i) => (
        <span key={i} style={{ display: "inline-flex", alignItems: "center" }}>
          <i className="codicon codicon-chevron-right breadcrumbs-sep" />
          <span className="breadcrumbs-seg">
            {i === segs.length - 1 && <i className="codicon codicon-file" />}
            {seg}
          </span>
        </span>
      ))}
    </div>
  );
}

export default function EditorArea() {
  const tabs = useEditorStore((s) => s.tabs);
  const activeKey = useEditorStore((s) => s.activeKey);

  const active = tabs.find((t) => t.key === activeKey);

  return (
    <div className="editor-area">
      <TabBar />
      {active?.kind === "file" && <Breadcrumbs path={active.path} />}
      <div className="editor-body">
        {active?.kind === "settings" && <SettingsPane />}
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
