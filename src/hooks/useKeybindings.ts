import { useEffect } from "react";
import { useUiStore } from "../state/uiStore";
import { useEditorStore } from "../state/editorStore";
import { useTerminalStore } from "../state/terminalStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { runCommand } from "../commands";

function isTextInput(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable)
  );
}

/**
 * Global keybinding layer — mirrors core VSCode defaults.
 */
export function useKeybindings(): void {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      if (!ctrl) {
        if (e.key === "Escape") {
          const ui = useUiStore.getState();
          if (ui.contextMenu) {
            ui.closeContextMenu();
            return;
          }
          if (ui.inputDialog || ui.confirmDialog) {
            ui.closeInput();
            ui.closeConfirm();
            return;
          }
        }
        return;
      }

      const key = e.key.toLowerCase();
      const shift = e.shiftKey;

      if (shift && key === "p") {
        e.preventDefault();
        runCommand("workbench.action.showCommands");
        return;
      }
      if (!shift && key === "p") {
        e.preventDefault();
        runCommand("workbench.command.quickOpen");
        return;
      }
      if (shift && key === "e") {
        e.preventDefault();
        runCommand("workbench.view.explorer");
        return;
      }
      if (shift && key === "f") {
        e.preventDefault();
        runCommand("workbench.view.search");
        return;
      }
      if (shift && key === "g") {
        e.preventDefault();
        runCommand("workbench.view.scm");
        return;
      }
      if (shift && key === "m") {
        e.preventDefault();
        runCommand("workbench.panel.problems");
        return;
      }
      if (shift && (key === "`" || key === "~")) {
        e.preventDefault();
        runCommand("workbench.action.terminal.new");
        return;
      }
      if (key === "`" || key === "~") {
        e.preventDefault();
        runCommand("workbench.action.terminal.toggleTerminal");
        return;
      }

      switch (key) {
        case "b":
          e.preventDefault();
          runCommand("workbench.action.toggleSidebar");
          break;
        case "s":
          e.preventDefault();
          void useEditorStore.getState().save();
          break;
        case "w":
          if (!isTextInput(e.target)) {
            e.preventDefault();
            const s = useEditorStore.getState();
            if (s.activeKey) s.closeTab(s.activeKey);
          }
          break;
        case "n":
          if (!isTextInput(e.target)) {
            e.preventDefault();
            runCommand("workbench.action.files.newFile");
          }
          break;
        default:
          break;
      }

      void useTerminalStore;
      void useWorkspaceStore;
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
