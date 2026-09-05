/**
 * Dev-only Tauri API shim.
 *
 * Active ONLY when running under `vite dev` in a plain browser (no Tauri
 * runtime). It stubs `window.__TAURI_INTERNALS__` so the UI mounts and can
 * be visually inspected; all backend commands resolve to benign defaults.
 * The whole module is dead-code-eliminated from production builds
 * (import.meta.env.DEV is false), and it never runs inside the real
 * Tauri webview (which defines __TAURI_INTERNALS__ itself).
 */

interface internals {
  invoke: (cmd: string, args?: unknown) => Promise<unknown>;
  transformCallback: (cb: unknown) => unknown;
  metadata: { currentWindow: { label: string }; currentWebview: { label: string } };
}

const w = window as unknown as { __TAURI_INTERNALS__?: internals };

if (import.meta.env.DEV && !w.__TAURI_INTERNALS__) {
  const EMPTY_ARRAY_COMMANDS = new Set([
    "list_shells",
    "list_dir",
    "list_all_files",
    "git_status",
    "git_branch",
    "search",
    "list_extensions",
  ]);

  const internals: internals = {
    invoke: (cmd: string) => {
      if (EMPTY_ARRAY_COMMANDS.has(cmd)) return Promise.resolve([]);
      return Promise.resolve(null);
    },
    transformCallback: (cb: unknown) => cb,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
  };

  w.__TAURI_INTERNALS__ = internals;
}

export {};
