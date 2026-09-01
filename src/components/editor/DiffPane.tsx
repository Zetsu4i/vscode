import { useEffect, useRef } from "react";
import { monaco } from "../../monaco";
import { useEditorStore } from "../../state/editorStore";
import { languageForPath } from "../../util/paths";

export default function DiffPane({ path }: { path: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const diffRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);

  useEffect(() => {
    const state = useEditorStore.getState();
    const buf = state.buffers[path];
    const base = state.diffBase[path] ?? "";
    if (!containerRef.current) return;

    const lang = languageForPath(path);
    const original = monaco.editor.createModel(base, lang);
    const modified = monaco.editor.createModel(buf?.text ?? "", lang);

    const diff = monaco.editor.createDiffEditor(containerRef.current, {
      theme: "dark-plus",
      automaticLayout: true,
      readOnly: true,
      renderSideBySide: true,
      fontSize: 14,
      fontFamily: 'Consolas, "Courier New", monospace',
      minimap: { enabled: false },
      scrollBeyondLastLine: true,
    });
    diff.setModel({ original, modified });
    diffRef.current = diff;

    // Keep modified side in sync while the file is being edited elsewhere
    const unsub = useEditorStore.subscribe((s) => {
      const b = s.buffers[path];
      if (b && b.text !== modified.getValue()) {
        modified.setValue(b.text);
      }
    });

    return () => {
      unsub();
      diff.dispose();
      original.dispose();
      modified.dispose();
    };
  }, [path]);

  return <div ref={containerRef} className="monaco-container" />;
}
