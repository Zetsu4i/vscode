import { useEffect } from "react";
import { useUiStore } from "../state/uiStore";
import { useKeybindingStore, resolveBindings, eventKey } from "../keybindings/store";
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
 * Commands that must not fire while the user is typing in a text field
 * (matching the narrow guards the hardcoded layer used to apply).
 */
const NO_TEXT_INPUT = new Set([
  "workbench.action.closeActiveEditor",
  "workbench.action.splitEditor",
  "workbench.action.files.newFile",
]);

/**
 * Global keybinding layer — fully data-driven. Defaults come from the
 * command registry; the user's keybindings.json overrides them.
 */
export function useKeybindings(): void {
  useEffect(() => {
    void useKeybindingStore.getState().init();

    const onKeyDown = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;

      // Escape handling is UI-state, not a command.
      if (e.key === "Escape") {
        const ui = useUiStore.getState();
        if (ui.paletteOpen) return; // palette handles its own Escape
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

      const binding = eventKey(e);

      // Key capture mode (Keybindings editor is listening for the next key).
      const { captureFor, setCapture, rebind } = useKeybindingStore.getState();
      if (captureFor) {
        e.preventDefault();
        e.stopPropagation();
        if (binding === "escape") {
          setCapture(null);
        } else {
          void rebind(captureFor, binding).then(() => setCapture(null));
        }
        return;
      }

      const command = resolveBindings(useKeybindingStore.getState().userRules).get(binding);
      if (!command) return;

      // Alt-only combos: do not steal plain Alt presses, only combos that match.
      if (binding.startsWith("alt+") && !ctrl) {
        e.preventDefault();
        runCommand(command);
        return;
      }

      if (!ctrl && !binding.startsWith("alt+")) return; // defaults are Ctrl-based

      if (NO_TEXT_INPUT.has(command) && isTextInput(e.target)) return;

      e.preventDefault();
      runCommand(command);
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
