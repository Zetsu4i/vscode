import { useEffect, useState } from "react";
import { ipc, InstalledExtension } from "../../ipc";
import { useWorkspaceStore } from "../../state/workspaceStore";

export default function ExtensionsView() {
  const root = useWorkspaceStore((s) => s.root);
  const [extensions, setExtensions] = useState<InstalledExtension[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    ipc
      .listExtensions(root ?? undefined)
      .then((exts) => {
        setExtensions(exts);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, [root]);

  return (
    <div className="view">
      <div className="view-header">
        <span className="view-header-title">Extensions</span>
      </div>
      <div className="extensions-note">
        <p>
          <b>Rust-native extension system.</b> VSTauri runs extensions in a
          capability-based WASM sandbox (wasmtime runtime lands in Phase 3 —
          see docs/ROADMAP.md).
        </p>
        <p>
          Drop extensions into{" "}
          <code>~/.vstauri/extensions/&lt;publisher.name&gt;/extension.json</code>{" "}
          or <code>.vstauri/extensions/</code> inside your workspace.
        </p>
      </div>
      <div className="view-section-header">
        <i className="codicon codicon-chevron-down" />
        <span className="view-section-name">INSTALLED ({extensions.length})</span>
      </div>
      {!loaded ? (
        <div className="tree-loading">Loading...</div>
      ) : extensions.length === 0 ? (
        <div className="tree-empty">No extensions installed yet.</div>
      ) : (
        extensions.map((ext) => (
          <div key={ext.manifest.id} className="tree-row ext-row" title={ext.dir}>
            <i className="codicon codicon-extensions ext-icon" />
            <div className="ext-info">
              <div className="ext-name">
                {ext.manifest.name} <span className="ext-version">{ext.manifest.version}</span>
              </div>
              <div className="ext-desc">{ext.manifest.description || ext.manifest.id}</div>
            </div>
          </div>
        ))
      )}
    </div>
  );
}
