import { useEditorStore } from "../../state/editorStore";
import { useEditorStore as store } from "../../state/editorStore";
import { useUiStore } from "../../state/uiStore";
import { baseName } from "../../util/paths";

const SEV_ICONS: Record<number, { icon: string; cls: string }> = {
  1: { icon: "codicon-error", cls: "problem-error" },
  2: { icon: "codicon-warning", cls: "problem-warning" },
  3: { icon: "codicon-info", cls: "problem-info" },
  4: { icon: "codicon-light-bulb", cls: "problem-info" },
};

export default function ProblemsView() {
  const problems = useEditorStore((s) => s.problems);
  const openFile = store((s) => s.openFile);
  const requestReveal = useUiStore((s) => s.requestReveal);

  if (problems.length === 0) {
    return (
      <div className="problems-empty">
        <i className="codicon codicon-check-all" />
        <span>No problems have been detected in the workspace.</span>
      </div>
    );
  }

  const byFile = new Map<string, typeof problems>();
  for (const p of problems) {
    const arr = byFile.get(p.path);
    if (arr) arr.push(p);
    else byFile.set(p.path, [p]);
  }

  return (
    <div className="problems-view">
      {[...byFile.entries()].map(([path, items]) => (
        <div key={path}>
          <div className="problems-file">
            <i className="codicon codicon-file" />
            <span>{baseName(path)}</span>
            <span className="problems-file-path">{path}</span>
          </div>
          {items.map((p, i) => {
            const sev = SEV_ICONS[p.severity] ?? SEV_ICONS[1];
            return (
              <div
                key={i}
                className="problem-row"
                onClick={() => {
                  void openFile(path).then(() =>
                    requestReveal(path, p.line, p.col)
                  );
                }}
              >
                <i className={`codicon ${sev.icon} ${sev.cls}`} />
                <span className="problem-message">{p.message}</span>
                {p.source && <span className="problem-source">{p.source}</span>}
                <span className="problem-loc">
                  [{p.line}, {p.col}]
                </span>
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}
