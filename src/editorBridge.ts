import type * as monaco from "monaco-editor";

/**
 * Bridge between the menu bar / command palette and the active Monaco
 * editor instance. MonacoPane registers the focused standalone editor
 * here so global commands (Edit menu, Go to Line, formatting...) can
 * drive it without coupling the menu layer to the editor internals.
 */

let activeEditor: monaco.editor.IStandaloneCodeEditor | null = null;

export function setActiveEditor(
  editor: monaco.editor.IStandaloneCodeEditor | null
): void {
  activeEditor = editor;
}

export function getActiveEditor(): monaco.editor.IStandaloneCodeEditor | null {
  return activeEditor;
}

/** Run a built-in Monaco action on the active editor if one exists. */
export function editorAction(actionId: string): void {
  const ed = activeEditor;
  if (!ed) return;
  ed.focus();
  ed.getAction(actionId)?.run();
}

export function editorUndo(): void {
  editorAction("undo");
}

export function editorRedo(): void {
  editorAction("redo");
}

export function editorCut(): void {
  editorAction("editor.action.clipboardCutAction");
}

export function editorCopy(): void {
  editorAction("editor.action.clipboardCopyAction");
}

export function editorPaste(): void {
  editorAction("editor.action.clipboardPasteAction");
}

export function editorFind(): void {
  editorAction("actions.find");
}

export function editorReplace(): void {
  editorAction("editor.action.startFindReplaceAction");
}

export function editorFormat(): void {
  editorAction("editor.action.formatDocument");
}

export function editorSelectAll(): void {
  editorAction("editor.action.selectAll");
}

/** Ask for a 1-based line[:column] and reveal it in the active editor. */
export function editorGoToLineCol(): void {
  void (async () => {
    const { useUiStore } = await import("./state/uiStore");
    const ui = useUiStore.getState();
    if (!activeEditor) {
      ui.showConfirm({
        title: "Go to Line/Column",
        message: "Open a file in the editor first.",
      });
      return;
    }
    ui.showInput({
      title: "Go to Line/Column",
      value: "",
      placeholder: "Line:Column (e.g. 42:7)",
      onOk: (value) => {
        const ed = activeEditor;
        if (!ed) return;
        const m = value.trim().match(/^(\d+)(?::(\d+))?$/);
        if (!m) return;
        const line = Math.max(1, parseInt(m[1], 10));
        const col = Math.max(1, parseInt(m[2] ?? "1", 10));
        ed.revealLineInCenter(line);
        ed.setPosition({ lineNumber: line, column: col });
        ed.focus();
      },
    });
  })();
}
