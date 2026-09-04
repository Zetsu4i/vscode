import { open } from "@tauri-apps/plugin-dialog";
import { useUiStore } from "./state/uiStore";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useEditorStore, selectActiveKey } from "./state/editorStore";
import { useTerminalStore } from "./state/terminalStore";
import { useSearchStore } from "./state/searchStore";
import { useSettingsStore } from "./state/settingsStore";

export interface Command {
  id: string;
  title: string;
  category: string;
  keybinding?: string;
  run: () => void | Promise<void>;
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
      const key = selectActiveKey(s);
      if (key) s.closeTab(key);
    },
  },
  {
    id: "workbench.action.splitEditor",
    title: "Split Editor Right",
    category: "View",
    keybinding: "Ctrl+\\",
    run: () => useEditorStore.getState().splitGroup("right"),
  },
  {
    id: "workbench.action.splitEditorDown",
    title: "Split Editor Down",
    category: "View",
    run: () => useEditorStore.getState().splitGroup("down"),
  },
  {
    id: "workbench.action.closeGroup",
    title: "Close Editor Group",
    category: "View",
    run: () => {
      const s = useEditorStore.getState();
      s.closeGroup(s.activeGroupId);
    },
  },
  {
    id: "workbench.action.focusNextGroup",
    title: "Focus Next Editor Group",
    category: "View",
    run: () => {
      const s = useEditorStore.getState();
      const idx = s.groups.findIndex((g) => g.id === s.activeGroupId);
      const next = s.groups[(idx + 1) % s.groups.length];
      if (next) s.focusGroup(next.id);
    },
  },
  {
    id: "workbench.action.moveEditorToNextGroup",
    title: "Move Editor into Next Group",
    category: "View",
    run: () => {
      const s = useEditorStore.getState();
      const key = selectActiveKey(s);
      if (!key) return;
      const idx = s.groups.findIndex((g) => g.id === s.activeGroupId);
      const next = s.groups[(idx + 1) % s.groups.length];
      if (next) s.moveTabToGroup(key, s.activeGroupId, next.id);
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
    id: "workbench.action.toggleBreadcrumbs",
    title: "Toggle Breadcrumbs",
    category: "View",
    run: () => useSettingsStore.getState().toggleBreadcrumbs(),
  },
  {
    id: "workbench.action.toggleStickyScroll",
    title: "Toggle Sticky Scroll",
    category: "View",
    run: () => useSettingsStore.getState().toggleStickyScroll(),
  },
  {
    id: "editor.action.fontZoomIn",
    title: "Font Zoom In",
    category: "Editor",
    keybinding: "Ctrl+=",
    run: () => useSettingsStore.getState().increaseFontSize(),
  },
  {
    id: "editor.action.fontZoomOut",
    title: "Font Zoom Out",
    category: "Editor",
    keybinding: "Ctrl+-",
    run: () => useSettingsStore.getState().decreaseFontSize(),
  },
  {
    id: "editor.action.fontZoomReset",
    title: "Font Zoom Reset",
    category: "Editor",
    run: () => useSettingsStore.getState().resetFontSize(),
  },
  {
    id: "editor.action.toggleMinimap",
    title: "Toggle Minimap",
    category: "Editor",
    run: () => useSettingsStore.getState().toggleMinimap(),
  },
  {
    id: "editor.action.toggleWordWrap",
    title: "Toggle Word Wrap",
    category: "Editor",
    keybinding: "Alt+Z",
    run: () => useSettingsStore.getState().toggleWordWrap(),
  },
  {
    id: "editor.action.toggleRenderWhitespace",
    title: "Toggle Render Whitespace",
    category: "Editor",
    run: () => useSettingsStore.getState().cycleRenderWhitespace(),
  },
  {
    id: "editor.action.toggleLigatures",
    title: "Toggle Font Ligatures",
    category: "Editor",
    run: () => useSettingsStore.getState().toggleLigatures(),
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
