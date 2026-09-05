import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useKeybindings } from "./hooks/useKeybindings";
import { useUiStore } from "./state/uiStore";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useEditorStore } from "./state/editorStore";
import { useTerminalStore } from "./state/terminalStore";
import { useSearchStore } from "./state/searchStore";
import { ipc, onFsChanged, onSearchProgress, onSearchDone } from "./ipc";
import { initSettingsAppliers } from "./settings/appliers";
import { useSettingsStore } from "./settings/settingsStore";
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
    // Settings: appliers first (they subscribe), then load — user/workspace
    // settings.json drive editor options, theme and auto save from boot.
    initSettingsAppliers();
    void useSettingsStore.getState().load();

    const unFs = onFsChanged((paths) => {
      void useWorkspaceStore.getState().refreshAll();
      void useGitStoreRefresh();
      // Editing settings.json in the editor must live-reload the engine.
      void useSettingsStore.getState().maybeReload(paths);
    });
    const unProg = onSearchProgress((p) => useSearchStore.getState().setProgress(p));
    const unDone = onSearchDone((p) => useSearchStore.getState().finish(p));

    return () => {
      void unFs.then((f) => f());
      void unProg.then((f) => f());
      void unDone.then((f) => f());
    };
  }, []);

  // Window-state: the window starts hidden (visible:false in tauri.conf.json)
  // so the backend can restore its saved geometry before anything is shown.
  // After the first painted frame we reveal the window (no white flash), then
  // keep saving geometry (debounced) as the user resizes/moves it.
  useEffect(() => {
    const win = getCurrentWindow();
    const raf = requestAnimationFrame(() => {
      void ipc.windowReady().catch(() => {
        /* window already visible (dev mode) */
      });
    });

    let timer: ReturnType<typeof setTimeout> | null = null;
    let disposed = false;
    const save = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(async () => {
        if (disposed) return;
        try {
          if (await win.isMaximized()) return; // keep last normal geometry
          const [scale, size, pos] = await Promise.all([
            win.scaleFactor(),
            win.outerSize(),
            win.outerPosition(),
          ]);
          if (!scale) return;
          await ipc.saveWindowState({
            width: size.width / scale,
            height: size.height / scale,
            x: pos.x / scale,
            y: pos.y / scale,
            maximized: false,
          });
        } catch {
          /* transient API errors are fine */
        }
      }, 350);
    };
    const unsubs: Array<() => void> = [];
    void win.onResized(save).then((f) => unsubs.push(f));
    void win.onMoved(save).then((f) => unsubs.push(f));

    return () => {
      disposed = true;
      cancelAnimationFrame(raf);
      if (timer) clearTimeout(timer);
      unsubs.forEach((f) => f());
    };
  }, []);

  // Window title follows VSCode convention: file - folder - VSTauri
  useEffect(() => {
    const unsub = useEditorStore.subscribe((state) => {
      const rootName = useWorkspaceStore.getState().rootName;
      const active = state.tabs.find((t) => t.key === state.activeKey);
      const parts: string[] = [];
      if (active) {
        parts.push(active.kind === "settings" ? "Settings" : baseName(active.path));
      }
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
