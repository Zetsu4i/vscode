import { useEffect, useRef } from "react";
import { monaco } from "../../monaco";
import { ipc, onLspDiagnostics, LspDiagnostic } from "../../ipc";
import { useEditorStore } from "../../state/editorStore";
import { useUiStore } from "../../state/uiStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { languageForPath } from "../../util/paths";

// ---- module-level singletons ----------------------------------------------

let editor: monaco.editor.IStandaloneCodeEditor | null = null;
const models = new Map<string, monaco.editor.ITextModel>();
const viewStates = new Map<string, monaco.editor.ICodeEditorViewState | null>();
let providersRegistered = false;
let diagnosticsListenerBound = false;

function uriToPath(uri: string): string {
  let p = uri.startsWith("file://") ? uri.slice("file://".length) : uri;
  try {
    p = decodeURIComponent(p);
  } catch {
    /* keep raw */
  }
  return p;
}

function mapKind(kind?: number): monaco.languages.CompletionItemKind {
  const k = kind ?? 1;
  const map: Record<number, monaco.languages.CompletionItemKind> = {
    1: monaco.languages.CompletionItemKind.Text,
    2: monaco.languages.CompletionItemKind.Method,
    3: monaco.languages.CompletionItemKind.Function,
    4: monaco.languages.CompletionItemKind.Constructor,
    5: monaco.languages.CompletionItemKind.Field,
    6: monaco.languages.CompletionItemKind.Variable,
    7: monaco.languages.CompletionItemKind.Class,
    8: monaco.languages.CompletionItemKind.Interface,
    9: monaco.languages.CompletionItemKind.Module,
    10: monaco.languages.CompletionItemKind.Property,
    11: monaco.languages.CompletionItemKind.Unit,
    12: monaco.languages.CompletionItemKind.Value,
    13: monaco.languages.CompletionItemKind.Enum,
    14: monaco.languages.CompletionItemKind.Keyword,
    15: monaco.languages.CompletionItemKind.Snippet,
    16: monaco.languages.CompletionItemKind.Color,
    17: monaco.languages.CompletionItemKind.File,
    18: monaco.languages.CompletionItemKind.Reference,
    19: monaco.languages.CompletionItemKind.Folder,
    20: monaco.languages.CompletionItemKind.EnumMember,
    21: monaco.languages.CompletionItemKind.Constant,
    22: monaco.languages.CompletionItemKind.Struct,
    23: monaco.languages.CompletionItemKind.Event,
    24: monaco.languages.CompletionItemKind.Operator,
    25: monaco.languages.CompletionItemKind.TypeParameter,
  };
  return map[k] ?? monaco.languages.CompletionItemKind.Text;
}

function severityToMonaco(s?: number): monaco.MarkerSeverity {
  switch (s) {
    case 1:
      return monaco.MarkerSeverity.Error;
    case 2:
      return monaco.MarkerSeverity.Warning;
    case 3:
      return monaco.MarkerSeverity.Info;
    case 4:
      return monaco.MarkerSeverity.Hint;
    default:
      return monaco.MarkerSeverity.Error;
  }
}

function registerProviders(): void {
  if (providersRegistered) return;
  providersRegistered = true;

  // Word-based suggestions for everything (works even without an LSP)
  monaco.languages.typescript?.typescriptDefaults?.setCompilerOptions({
    allowNonTsExtensions: true,
    allowJs: true,
    target: monaco.languages.typescript.ScriptTarget.ES2020,
  });

  monaco.languages.registerCompletionItemProvider("*", {
    triggerCharacters: [".", ":", "<", "/", '"', "@", "#"],
    async provideCompletionItems(model, position) {
      const path = uriToPath(model.uri.toString());
      const lang = languageForPath(path);
      const root = useWorkspaceStore.getState().root;
      if (!lang || !root) return { suggestions: [] };
      try {
        const items = await ipc.lspCompletion(
          lang,
          path,
          position.lineNumber - 1,
          position.column - 1
        );
        const word = model.getWordUntilPosition(position);
        const range: monaco.IRange = {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endColumn: word.endColumn,
        };
        const suggestions: monaco.languages.CompletionItem[] = items.map((it) => {
          const label =
            (it["label"] as string) ?? (it["newText"] as string) ?? "";
          const insert = (it["insertText"] as string) ?? label;
          return {
            label,
            kind: mapKind(it["kind"] as number),
            insertText: insert,
            range,
            detail: it["detail"] as string | undefined,
            documentation:
              (it["documentation"] as string | undefined) ?? undefined,
          };
        });
        return { suggestions };
      } catch {
        return { suggestions: [] };
      }
    },
  });

  monaco.languages.registerHoverProvider("*", {
    async provideHover(model, position) {
      const path = uriToPath(model.uri.toString());
      const lang = languageForPath(path);
      const root = useWorkspaceStore.getState().root;
      if (!lang || !root) return null;
      try {
        const hover = (await ipc.lspHover(
          lang,
          path,
          position.lineNumber - 1,
          position.column - 1
        )) as {
          contents?: { value?: string }[] | { value?: string };
        } | null;
        if (!hover?.contents) return null;
        const raw = Array.isArray(hover.contents)
          ? hover.contents.map((c) => c?.value ?? "").join("\n\n")
          : (hover.contents.value ?? "");
        if (!raw) return null;
        return { range: undefined, contents: [{ value: raw }] };
      } catch {
        return null;
      }
    },
  });
}

function applyDiagnostics(payload: {
  uri: string;
  diagnostics: LspDiagnostic[];
}): void {
  const path = uriToPath(payload.uri);
  const model = models.get(path);
  const markers: monaco.editor.IMarkerData[] = payload.diagnostics.map((d) => ({
    severity: severityToMonaco(d.severity),
    startLineNumber: d.range.start.line + 1,
    startColumn: d.range.start.character + 1,
    endLineNumber: d.range.end.line + 1,
    endColumn: d.range.end.character + 1,
    message: d.message,
    source: d.source,
  }));
  if (model) {
    monaco.editor.setModelMarkers(model, "lsp", markers);
  }
  const problems = useEditorStore
    .getState()
    .problems.filter((p) => p.path !== path)
    .concat(
      markers.map((m) => ({
        path,
        line: m.startLineNumber,
        col: m.startColumn,
        endLine: m.endLineNumber,
        endCol: m.endColumn,
        message: m.message,
        severity:
          m.severity === monaco.MarkerSeverity.Error
            ? 1
            : m.severity === monaco.MarkerSeverity.Warning
              ? 2
              : 3,
        source: m.source,
      }))
    );
  useEditorStore.getState().setProblems(problems);
}

function getModel(path: string, text: string, lang: string): monaco.editor.ITextModel {
  let model = models.get(path);
  if (!model) {
    model = monaco.editor.createModel(text, lang, monaco.Uri.file(path));
    models.set(path, model);
    model.onWillDispose(() => models.delete(path));
  }
  return model;
}

async function ensureLspFor(path: string, text: string): Promise<void> {
  const lang = languageForPath(path);
  if (!lang) return;
  const root = useWorkspaceStore.getState().root;
  const ui = useUiStore.getState();
  if (!root) return;
  try {
    const status = await ipc.lspStatus(lang);
    if (status !== "running") {
      await ipc.lspStart(root, lang);
      ui.setLspStatus(lang, "running");
    }
    await ipc.lspDidOpen(lang, path, text, 1);
  } catch {
    ui.setLspStatus(lang, "unavailable");
  }
}

// ---- component -------------------------------------------------------------

export default function MonacoPane({ path }: { path: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const switching = useRef(false);
  const setActiveTabPath = path;

  useEffect(() => {
    registerProviders();

    if (!editor && containerRef.current) {
      editor = monaco.editor.create(containerRef.current, {
        model: null,
        theme: "dark-plus",
        automaticLayout: true,
        fontSize: 14,
        fontFamily:
          'Consolas, "Courier New", "Droid Sans Mono", monospace',
        minimap: { enabled: true, renderCharacters: true },
        scrollBeyondLastLine: true,
        smoothScrolling: true,
        cursorBlinking: "blink",
        tabSize: 4,
        renderWhitespace: "selection",
        guides: { indentation: true, bracketPairs: false },
        bracketPairColorization: { enabled: true },
        wordBasedSuggestions: "currentDocument",
        quickSuggestions: { other: true, comments: false, strings: false },
        suggestSelection: "first",
        lineNumbersMinChars: 4,
        padding: { top: 8 },
      });

      editor.onDidChangeCursorPosition((e) => {
        useUiStore
          .getState()
          .setCursor(e.position.lineNumber, e.position.column);
      });
    }

    if (!diagnosticsListenerBound) {
      diagnosticsListenerBound = true;
      onLspDiagnostics(applyDiagnostics);
    }

    return () => {
      if (editor) {
        if (editor.getModel()) {
          const p = uriToPath(editor.getModel()!.uri.toString());
          viewStates.set(p, editor.saveViewState());
        }
      }
    };
  }, []);

  // Switch models when the active file changes
  useEffect(() => {
    const ed = editor;
    if (!ed || !containerRef.current) return;
    const buf = useEditorStore.getState().buffers[path];
    if (!buf) return;

    const lang = languageForPath(path);
    const model = getModel(path, buf.text, lang);
    switching.current = true;

    const current = ed.getModel();
    if (current && current !== model) {
      const p = uriToPath(current.uri.toString());
      viewStates.set(p, ed.saveViewState());
    }
    ed.setModel(model);
    const vs = viewStates.get(path);
    if (vs) ed.restoreViewState(vs);
    ed.focus();

    // LSP: open document (only once per open — didOpen with version 1)
    const isFirstOpen = buf.version === 1;
    if (isFirstOpen) {
      void ensureLspFor(path, buf.text);
    }

    // Content change listener (attached once per model creation path)
    const disp = model.onDidChangeContent(() => {
      if (switching.current) return;
      const text = model.getValue();
      useEditorStore.getState().setText(path, text);
      const b = useEditorStore.getState().buffers[path];
      if (b) {
        const root = useWorkspaceStore.getState().root;
        const langId = languageForPath(path);
        if (root && langId) {
          // Debounced didChange
          window.clearTimeout(
            (model as unknown as { _lspTimer?: number })._lspTimer
          );
          (model as unknown as { _lspTimer?: number })._lspTimer =
            window.setTimeout(() => {
              void ipc
                .lspDidChange(langId, path, b.text, b.version)
                .catch(() => {});
            }, 400);
        }
      }
    });
    switching.current = false;

    return () => disp.dispose();
  }, [path]);

  // Ctrl+S handled globally; keep buffer in sync after save
  useEffect(() => {
    const unsub = useEditorStore.subscribe((state, prev) => {
      const b = state.buffers[path];
      const pb = prev?.buffers?.[path];
      if (b && pb && pb.dirty && !b.dirty && editor?.getModel()) {
        // saved — LSP didSave will be added in Phase 2
      }
    });
    return () => unsub();
  }, [path]);

  // Reveal requests (search results, problems)
  useEffect(() => {
    const unsub = useUiStore.subscribe((state) => {
      const r = state.reveal;
      if (r && r.path === setActiveTabPath && editor) {
        editor.revealLineInCenter(r.line);
        editor.setPosition({ lineNumber: r.line, column: r.col });
        editor.focus();
      }
    });
    return () => unsub();
  }, [setActiveTabPath]);

  const buf = useEditorStore((s) => s.buffers[path]);

  if (buf?.binary) {
    return (
      <div className="binary-view">
        <i className="codicon codicon-file-binary" />
        <p>The file is not displayed in the text editor because it is binary.</p>
        <p className="muted">{path}</p>
      </div>
    );
  }

  return <div ref={containerRef} className="monaco-container" />;
}
