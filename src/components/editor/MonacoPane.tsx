import { useEffect, useRef } from "react";
import { monaco } from "../../monaco";
import { ipc, onLspDiagnostics, LspDiagnostic } from "../../ipc";
import { useEditorStore } from "../../state/editorStore";
import { useUiStore } from "../../state/uiStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useSettingsStore } from "../../state/settingsStore";
import { languageForPath } from "../../util/paths";

// ---- module-level singletons (shared across editor groups) -----------------

const models = new Map<string, monaco.editor.ITextModel>();
const viewStates = new Map<string, monaco.editor.ICodeEditorViewState | null>();
const hookedModels = new WeakSet<monaco.editor.ITextModel>();
const lspOpened = new Set<string>();
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

/**
 * Attach store-sync + LSP hooks exactly once per model, so multiple editor
 * groups viewing the same file never double-fire events.
 */
function attachModelHooks(model: monaco.editor.ITextModel, path: string): void {
  if (hookedModels.has(model)) return;
  hookedModels.add(model);

  model.onDidChangeContent((e) => {
    const text = model.getValue();
    useEditorStore.getState().setText(path, text);
    const b = useEditorStore.getState().buffers[path];
    const root = useWorkspaceStore.getState().root;
    const langId = languageForPath(path);
    if (b && root && langId) {
      // Incremental sync: forward Monaco's ranged edits as LSP content changes.
      // Monaco orders changes from the end of the document backwards, which
      // matches LSP's sequential application semantics.
      const changes = e.changes.map((c) => ({
        range: {
          start: { line: c.range.startLineNumber - 1, character: c.range.startColumn - 1 },
          end: { line: c.range.endLineNumber - 1, character: c.range.endColumn - 1 },
        },
        text: c.text,
      }));
      window.clearTimeout(
        (model as unknown as { _lspTimer?: number })._lspTimer
      );
      (model as unknown as { _lspTimer?: number })._lspTimer = window.setTimeout(
        () => {
          void ipc
            .lspDidChange(langId, path, changes, b.version)
            .catch(() => {});
        },
        400
      );
    }
  });

  model.onWillDispose(() => {
    models.delete(path);
    lspOpened.delete(path);
    window.clearTimeout(
      (model as unknown as { _lspTimer?: number })._lspTimer
    );
  });
}

function getModel(path: string, text: string, lang: string): monaco.editor.ITextModel {
  let model = models.get(path);
  if (!model) {
    model = monaco.editor.createModel(text, lang, monaco.Uri.file(path));
    models.set(path, model);
    attachModelHooks(model, path);
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
    if (!lspOpened.has(path)) {
      lspOpened.add(path);
      await ipc.lspDidOpen(lang, path, text, 1);
    }
  } catch {
    ui.setLspStatus(lang, "unavailable");
  }
}

// ---- component (one instance per editor group) -------------------------------

export default function MonacoPane({ path }: { path: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const switching = useRef(false);

  useEffect(() => {
    registerProviders();

    if (!diagnosticsListenerBound) {
      diagnosticsListenerBound = true;
      onLspDiagnostics(applyDiagnostics);
    }

    if (!editorRef.current && containerRef.current) {
      const ed = monaco.editor.create(containerRef.current, {
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
        stickyScroll: { enabled: useSettingsStore.getState().stickyScroll },
      });
      editorRef.current = ed;

      ed.onDidChangeCursorPosition((e) => {
        // Only the focused group updates the status bar cursor readout.
        if (ed.hasTextFocus()) {
          useUiStore
            .getState()
            .setCursor(e.position.lineNumber, e.position.column);
        }
      });

      ed.onDidBlurEditorText(() => {
        const model = ed.getModel();
        if (model) {
          viewStates.set(uriToPath(model.uri.toString()), ed.saveViewState());
        }
      });
    }

    return () => {
      const ed = editorRef.current;
      if (ed) {
        const model = ed.getModel();
        if (model) {
          viewStates.set(uriToPath(model.uri.toString()), ed.saveViewState());
        }
        ed.dispose();
        editorRef.current = null;
      }
    };
  }, []);

  // Switch models when this group's active file changes
  useEffect(() => {
    const ed = editorRef.current;
    if (!ed || !containerRef.current) return;
    const buf = useEditorStore.getState().buffers[path];
    if (!buf) return;

    const lang = languageForPath(path);
    const model = getModel(path, buf.text, lang);
    switching.current = true;

    ed.setModel(model);
    const vs = viewStates.get(path);
    if (vs) ed.restoreViewState(vs);
    ed.focus();

    // LSP: open document once per model lifetime
    if (buf.version === 1 && !lspOpened.has(path)) {
      void ensureLspFor(path, buf.text);
    }
    switching.current = false;
  }, [path]);

  // Buffer reload after external events (save-all, replace-all, revert)
  useEffect(() => {
    const unsub = useEditorStore.subscribe((state, prev) => {
      const b = state.buffers[path];
      const pb = prev?.buffers?.[path];
      const ed = editorRef.current;
      if (!b || !pb || !ed) return;
      const model = ed.getModel();
      if (!model || uriToPath(model.uri.toString()) !== path) return;
      // Push store text into the model only when the change did not originate
      // from this model itself (model.getValue() already matches in that case).
      if (b.text !== model.getValue()) {
        switching.current = true;
        model.pushEditOperations(
          [],
          [{ range: model.getFullModelRange(), text: b.text }],
          () => null
        );
        switching.current = false;
      }
    });
    return () => unsub();
  }, [path]);

  // Reveal requests (search results, problems)
  useEffect(() => {
    const unsub = useUiStore.subscribe((state) => {
      const r = state.reveal;
      const ed = editorRef.current;
      if (r && ed && r.path === path) {
        const model = ed.getModel();
        if (model && uriToPath(model.uri.toString()) === path) {
          ed.revealLineInCenter(r.line);
          ed.setPosition({ lineNumber: r.line, column: r.col });
          ed.focus();
        }
      }
    });
    return () => unsub();
  }, [path]);

  // Editor settings reactively applied (sticky scroll, breadcrumbs affect layout only)
  useEffect(() => {
    const unsub = useSettingsStore.subscribe((s) => {
      editorRef.current?.updateOptions({
        stickyScroll: { enabled: s.stickyScroll },
      });
    });
    return () => unsub();
  }, []);

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
