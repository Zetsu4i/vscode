import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useUiStore } from "./state/uiStore";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useEditorStore } from "./state/editorStore";
import { useTerminalStore } from "./state/terminalStore";
import { useSearchStore } from "./state/searchStore";
import {
  editorAction,
  editorFind,
  editorFormat,
  editorGoToLineCol,
  editorRedo,
  editorReplace,
  editorUndo,
} from "./editorBridge";

export interface Command {
  id: string;
  title: string;
  category: string;
  keybinding?: string;
  run: () => void | Promise<void>;
}

/* Window zoom (borrowed from SideX's approach: webview zoom instead of
   per-element scaling) with VSCode-style bounded steps. */
let zoomFactor = 1;

async function setZoom(factor: number): Promise<void> {
  zoomFactor = Math.min(3, Math.max(0.5, factor));
  try {
    await getCurrentWebview().setZoom(zoomFactor);
  } catch (e) {
    console.error("zoom failed", e);
  }
}

async function pickAndOpenFolder(): Promise<void> {
  const dir = await open({ directory: true, multiple: false, title: "Open Folder" });
  if (typeof dir === "string" && dir) {
    await useWorkspaceStore.getState().openFolder(dir);
  }
}

export const commands: Command[] = [
  {
    id: "workbench.action.openFolder",
    title: "Open Folder...",
    category: "File",
    run: pickAndOpenFolder,
  },
  {
    id: "workbench.action.files.save",
    title: "Save",
    category: "File",
    keybinding: "Ctrl+S",
    run: () => useEditorStore.getState().save(),
  },
  {
    id: "workbench.action.files.saveAll",
    title: "Save All",
    category: "File",
    run: () => useEditorStore.getState().saveAll(),
  },
  {
    id: "workbench.action.closeActiveEditor",
    title: "Close Editor",
    category: "View",
    keybinding: "Ctrl+W",
    run: () => {
      const s = useEditorStore.getState();
      if (s.activeKey) s.closeTab(s.activeKey);
    },
  },
  {
    id: "workbench.action.closeAllEditors",
    title: "Close All Editors",
    category: "View",
    run: () => useEditorStore.getState().closeAll(),
  },
  {
    id: "workbench.action.files.newFile",
    title: "New File...",
    category: "File",
    run: () => {
      const ws = useWorkspaceStore.getState();
      const ui = useUiStore.getState();
      if (!ws.root) {
        ui.showConfirm({
          title: "No folder open",
          message: "Open a folder first to create files.",
        });
        return;
      }
      ui.showInput({
        title: "New File (relative path)",
        value: "",
        placeholder: "src/example.ts",
        onOk: async (rel) => {
          const { joinPath } = await import("./util/paths");
          const full = joinPath(ws.root!, rel);
          try {
            await (await import("./ipc")).ipc.createFile(full);
            await ws.refreshAll();
            useEditorStore.getState().openFile(full);
          } catch (e) {
            console.error(e);
          }
        },
      });
    },
  },
  {
    id: "workbench.action.toggleSidebar",
    title: "Toggle Primary Side Bar",
    category: "View",
    keybinding: "Ctrl+B",
    run: () => useUiStore.getState().toggleSidebar(),
  },
  {
    id: "workbench.action.terminal.toggleTerminal",
    title: "Toggle Terminal",
    category: "View",
    keybinding: "Ctrl+`",
    run: async () => {
      const ui = useUiStore.getState();
      const ts = useTerminalStore.getState();
      if (!ui.panelVisible) {
        ui.setPanelTab("terminal");
        if (ts.terms.length === 0) {
          const ws = useWorkspaceStore.getState();
          await ts.create(ws.root ?? undefined);
        }
      } else {
        ui.togglePanel();
      }
    },
  },
  {
    id: "workbench.action.terminal.new",
    title: "Create New Terminal",
    category: "Terminal",
    run: async () => {
      const ws = useWorkspaceStore.getState();
      await useTerminalStore.getState().create(ws.root ?? undefined);
      useUiStore.getState().setPanelTab("terminal");
    },
  },
  {
    id: "workbench.view.explorer",
    title: "Show Explorer",
    category: "View",
    keybinding: "Ctrl+Shift+E",
    run: () => useUiStore.getState().setView("explorer"),
  },
  {
    id: "workbench.view.search",
    title: "Show Search",
    category: "View",
    keybinding: "Ctrl+Shift+F",
    run: () => useUiStore.getState().setView("search"),
  },
  {
    id: "workbench.view.scm",
    title: "Show Source Control",
    category: "View",
    keybinding: "Ctrl+Shift+G",
    run: () => useUiStore.getState().setView("git"),
  },
  {
    id: "workbench.view.extensions",
    title: "Show Extensions",
    category: "View",
    run: () => useUiStore.getState().setView("extensions"),
  },
  {
    id: "workbench.panel.problems",
    title: "Show Problems",
    category: "View",
    keybinding: "Ctrl+Shift+M",
    run: () => useUiStore.getState().setPanelTab("problems"),
  },
  {
    id: "workbench.command.quickOpen",
    title: "Go to File...",
    category: "Go",
    keybinding: "Ctrl+P",
    run: () => useUiStore.getState().openPalette("files"),
  },
  {
    id: "workbench.action.showCommands",
    title: "Show All Commands",
    category: "View",
    keybinding: "Ctrl+Shift+P",
    run: () => useUiStore.getState().openPalette("commands"),
  },
  {
    id: "workbench.action.findInFiles",
    title: "Find in Files",
    category: "Search",
    run: () => {
      useUiStore.getState().setView("search");
    },
  },
  {
    id: "search.action.runSearch",
    title: "Search: Run Query",
    category: "Search",
    run: () => useSearchStore.getState().run(),
  },
  {
    id: "git.refresh",
    title: "Git: Refresh",
    category: "Git",
    run: () => useWorkspaceStore.getState() && useUiStore.getState().setView("git"),
  },
  {
    id: "workbench.action.selectTheme",
    title: "Color Theme",
    category: "Preferences",
    keybinding: "Ctrl+K Ctrl+T",
    run: () => useUiStore.getState().openPalette("themes"),
  },
  {
    id: "workbench.action.openSettings",
    title: "Open Settings (UI)",
    category: "Preferences",
    keybinding: "Ctrl+,",
    run: () => useEditorStore.getState().openSettings(),
  },
  {
    id: "workbench.action.openSettingsJson",
    title: "Open User Settings (JSON)",
    category: "Preferences",
    run: async () => {
      const { ipc } = await import("./ipc");
      const p = await ipc.settingsPath("user", null);
      useEditorStore.getState().openFile(p);
    },
  },
  {
    id: "workbench.action.openWorkspaceSettingsJson",
    title: "Open Workspace Settings (JSON)",
    category: "Preferences",
    run: async () => {
      const root = useWorkspaceStore.getState().root;
      if (!root) {
        useUiStore.getState().showConfirm({
          title: "No folder open",
          message: "Open a folder first to edit workspace settings.",
        });
        return;
      }
      const { ipc } = await import("./ipc");
      const p = await ipc.settingsPath("workspace", root);
      useEditorStore.getState().openFile(p);
    },
  },
  {
    id: "workbench.action.togglePanel",
    title: "Toggle Panel",
    category: "View",
    keybinding: "Ctrl+J",
    run: () => useUiStore.getState().togglePanel(),
  },
  {
    id: "workbench.action.zoomIn",
    title: "Zoom In",
    category: "View",
    keybinding: "Ctrl+=",
    run: () => void setZoom(zoomFactor + 0.1),
  },
  {
    id: "workbench.action.zoomOut",
    title: "Zoom Out",
    category: "View",
    keybinding: "Ctrl+-",
    run: () => void setZoom(zoomFactor - 0.1),
  },
  {
    id: "workbench.action.zoomReset",
    title: "Reset Zoom",
    category: "View",
    keybinding: "Ctrl+0",
    run: () => void setZoom(1),
  },
  {
    id: "edit.action.undo",
    title: "Undo",
    category: "Edit",
    keybinding: "Ctrl+Z",
    run: editorUndo,
  },
  {
    id: "edit.action.redo",
    title: "Redo",
    category: "Edit",
    keybinding: "Ctrl+Y",
    run: editorRedo,
  },
  {
    id: "actions.find",
    title: "Find",
    category: "Edit",
    keybinding: "Ctrl+F",
    run: editorFind,
  },
  {
    id: "editor.action.startFindReplaceAction",
    title: "Replace",
    category: "Edit",
    keybinding: "Ctrl+H",
    run: editorReplace,
  },
  {
    id: "editor.action.formatDocument",
    title: "Format Document",
    category: "Edit",
    keybinding: "Shift+Alt+F",
    run: editorFormat,
  },
  {
    id: "editor.action.gotoLine",
    title: "Go to Line/Column...",
    category: "Go",
    keybinding: "Ctrl+G",
    run: editorGoToLineCol,
  },
  {
    id: "editor.action.commentLine",
    title: "Toggle Line Comment",
    category: "Edit",
    keybinding: "Ctrl+/",
    run: () => editorAction("editor.action.commentLine"),
  },
  {
    id: "workbench.action.reloadWindow",
    title: "Reload Window",
    category: "Developer",
    run: () => window.location.reload(),
  },
];

export function runCommand(id: string): void {
  const cmd = commands.find((c) => c.id === id);
  if (cmd) void cmd.run();
}

export { pickAndOpenFolder };
