import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useKeybindings } from "./hooks/useKeybindings";
import { useUiStore } from "./state/uiStore";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useEditorStore } from "./state/editorStore";
import { useTerminalStore } from "./state/terminalStore";
import { useSearchStore } from "./state/searchStore";
import { onFsChanged, onSearchProgress, onSearchDone } from "./ipc";
import { baseName } from "./util/paths";

import TitleBar from "./components/titlebar/TitleBar";
import ActivityBar from "./components/activitybar/ActivityBar";
import Sidebar from "./components/sidebar/Sidebar";
import EditorArea from "./components/editor/EditorArea";
import BottomPanel from "./components/panel/BottomPanel";
import StatusBar from "./components/statusbar/StatusBar";
import QuickOpen from "./components/palette/QuickOpen";
import ContextMenu from "./components/menus/ContextMenu";
import { InputDialog, ConfirmDialog } from "./components/dialogs/Dialogs";

export default function App() {
  useKeybindings();

  // Restore last workspace + wire global backend events
  useEffect(() => {
    void useWorkspaceStore.getState().initFromSaved();

    const unFs = onFsChanged(() => {
      void useWorkspaceStore.getState().refreshAll();
      void useGitStoreRefresh();
    });
    const unProg = onSearchProgress((p) => useSearchStore.getState().setProgress(p));
    const unDone = onSearchDone((p) => useSearchStore.getState().finish(p));

    return () => {
      void unFs.then((f) => f());
      void unProg.then((f) => f());
      void unDone.then((f) => f());
    };
  }, []);

  // Window title follows VSCode convention: file - folder - VSTauri
  useEffect(() => {
    const unsub = useEditorStore.subscribe((state) => {
      const rootName = useWorkspaceStore.getState().rootName;
      const active = state.tabs.find((t) => t.key === state.activeKey);
      const parts: string[] = [];
      if (active) parts.push(baseName(active.path));
      if (rootName) parts.push(rootName);
      parts.push("VSTauri");
      void getCurrentWindow().setTitle(parts.join(" - "));
    });
    return () => unsub();
  }, []);

  // First terminal spawns lazily when panel opens — pre-warm nothing here.

  return (
    <div className="workbench">
      <TitleBar />
      <div className="workbench-main">
        <ActivityBar />
        <SidebarWithVisibility />
        <div className="content-column">
          <EditorArea />
          <BottomPanel />
        </div>
      </div>
      <StatusBar />
      <QuickOpen />
      <ContextMenu />
      <InputDialog />
      <ConfirmDialog />
    </div>
  );
}

function SidebarWithVisibility() {
  const visible = useUiStore((s) => s.sidebarVisible);
  if (!visible) return null;
  return <Sidebar />;
}

async function useGitStoreRefresh(): Promise<void> {
  const { useGitStore } = await import("./state/gitStore");
  await useGitStore.getState().refresh();
}
