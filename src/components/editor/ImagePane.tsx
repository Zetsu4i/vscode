import { useEffect, useState } from "react";

const IMAGE_RE = /\.(png|jpe?g|gif|webp|bmp|ico|svg)$/i;

export function isImage(path: string): boolean {
  return IMAGE_RE.test(path);
}

/**
 * Image preview backed by the `vstauri://` asset protocol: the bytes are
 * streamed by the Rust backend with traversal guards (path must canonicalize
 * inside the registered workspace roots), so no file contents ever cross the
 * IPC boundary as JSON.
 */
export default function ImagePane({ path }: { path: string }) {
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [path]);

  if (failed) {
    return (
      <div className="image-pane-empty">
        <p>Preview is not available for this file.</p>
        <p className="image-pane-muted">{path}</p>
      </div>
    );
  }

  const src = `vstauri://localhost/file?path=${encodeURIComponent(path)}`;
  const name = path.split(/[\\/]/).pop() ?? path;

  return (
    <div className="image-pane">
      <div className="image-pane-toolbar">
        <span className="image-pane-name">{name}</span>
        <span className="image-pane-muted">vstauri asset preview</span>
      </div>
      <div className="image-pane-canvas">
        <img src={src} alt={name} onError={() => setFailed(true)} />
      </div>
    </div>
  );
}
