import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ---------- payload types ----------

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modifiedMs: number;
}

export interface FileContent {
  content: string;
  isBinary: boolean;
  truncated: boolean;
  size: number;
}

export interface SearchHit {
  file: string;
  lineNumber: number;
  text: string;
  ranges: [number, number][];
}

export interface GitChange {
  x: string;
  y: string;
  path: string;
  origPath: string | null;
}

export interface GitStatus {
  branch: string | null;
  changes: GitChange[];
}

export interface LogEntry {
  hash: string;
  subject: string;
}

export interface PtyInfo {
  id: number;
  shell: string;
}

export interface ExtensionManifest {
  id: string;
  name: string;
  publisher: string;
  version: string;
  description: string;
  main: string | null;
  activationEvents: string[];
  contributes: {
    commands: { command: string; title: string; category: string | null }[];
    themes: { label: string; path: string; kind: string }[];
    keybindings: { command: string; key: string }[];
  };
}

export interface InstalledExtension {
  manifest: ExtensionManifest;
  dir: string;
}

export interface LspDiagnostic {
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  message: string;
  severity?: number; // 1=Error 2=Warning 3=Info 4=Hint
  source?: string;
}

/** LSP TextDocumentContentChangeEvent — range omitted = full replace. */
export interface LspContentChange {
  range?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  text: string;
}

// ---------- command wrappers ----------

export const ipc = {
  // filesystem
  listDir: (path: string) => invoke<FileEntry[]>("list_dir", { path }),
  readFile: (path: string) => invoke<FileContent>("read_file", { path }),
  writeFile: (path: string, content: string) =>
    invoke<void>("write_file", { path, content }),
  createFile: (path: string) => invoke<void>("create_file", { path }),
  createDir: (path: string) => invoke<void>("create_dir", { path }),
  renamePath: (from: string, to: string) =>
    invoke<void>("rename_path", { from, to }),
  deletePath: (path: string, recursive = true) =>
    invoke<void>("delete_path", { path, recursive }),
  listAllFiles: (root: string, limit?: number) =>
    invoke<string[]>("list_all_files", { root, limit: limit ?? null }),
  fileExists: (path: string) => invoke<boolean>("file_exists", { path }),

  // watcher
  watchFolder: (root: string) => invoke<void>("watch_folder", { root }),

  // terminal
  createPty: (cwd?: string, shell?: string) =>
    invoke<PtyInfo>("create_pty", { cwd: cwd ?? null, shell: shell ?? null }),
  writePty: (id: number, data: string) => invoke<void>("write_pty", { id, data }),
  resizePty: (id: number, rows: number, cols: number) =>
    invoke<void>("resize_pty", { id, rows, cols }),
  killPty: (id: number) => invoke<void>("kill_pty", { id }),

  // search
  searchWorkspace: (
    root: string,
    query: string,
    isRegex: boolean,
    caseSensitive: boolean,
    wholeWord: boolean
  ) =>
    invoke<void>("search_workspace", {
      root,
      query,
      isRegex,
      caseSensitive,
      wholeWord,
    }),

  replaceAll: (
    root: string,
    query: string,
    replacement: string,
    isRegex: boolean,
    caseSensitive: boolean,
    wholeWord: boolean
  ) =>
    invoke<{ filesChanged: string[]; totalReplacements: number }>("replace_all", {
      root,
      query,
      replacement,
      isRegex,
      caseSensitive,
      wholeWord,
    }),

  // git
  gitIsRepo: (root: string) => invoke<boolean>("git_is_repo", { root }),
  gitStatus: (root: string) => invoke<GitStatus>("git_status", { root }),
  gitStage: (root: string, paths: string[]) =>
    invoke<void>("git_stage", { root, paths }),
  gitUnstage: (root: string, paths: string[]) =>
    invoke<void>("git_unstage", { root, paths }),
  gitCommit: (root: string, message: string) =>
    invoke<string>("git_commit", { root, message }),
  gitBranch: (root: string) => invoke<string | null>("git_branch", { root }),
  gitLog: (root: string, limit?: number) =>
    invoke<LogEntry[]>("git_log", { root, limit: limit ?? null }),
  gitShowHead: (root: string, path: string) =>
    invoke<string>("git_show_head", { root, path }),
  gitDiffFile: (root: string, path: string, staged: boolean) =>
    invoke<string>("git_diff_file", { root, path, staged }),

  // language servers
  lspStart: (root: string, language: string) =>
    invoke<string>("lsp_start", { root, language }),
  lspStop: (language: string) => invoke<void>("lsp_stop", { language }),
  lspStatus: (language: string) => invoke<string>("lsp_status", { language }),
  lspDidOpen: (language: string, path: string, text: string, version: number) =>
    invoke<void>("lsp_did_open", { language, path, text, version }),
  lspDidChange: (
    language: string,
    path: string,
    changes: LspContentChange[],
    version: number
  ) => invoke<void>("lsp_did_change", { language, path, changes, version }),
  lspDidSave: (language: string, path: string) =>
    invoke<void>("lsp_did_save", { language, path }),
  lspCompletion: (language: string, path: string, line: number, character: number) =>
    invoke<Record<string, unknown>[]>("lsp_completion", {
      language,
      path,
      line,
      character,
    }),
  lspHover: (language: string, path: string, line: number, character: number) =>
    invoke<unknown>("lsp_hover", { language, path, line, character }),

  // extensions
  listExtensions: (root?: string) =>
    invoke<InstalledExtension[]>("list_extensions", { root: root ?? null }),
};

// ---------- event listeners ----------

export function onFsChanged(cb: (paths: string[]) => void) {
  return listen<{ paths: string[] }>("fs-changed", (e) => cb(e.payload.paths));
}

export function onPtyOutput(id: number, cb: (b64: string) => void) {
  return listen<string>(`pty-output-${id}`, (e) => cb(e.payload));
}

export function onPtyExit(id: number, cb: () => void) {
  return listen<number>(`pty-exit-${id}`, () => cb());
}

export function onSearchProgress(
  cb: (p: { filesScanned: number; hits: number }) => void
) {
  return listen<{ filesScanned: number; hits: number }>(
    "search-progress",
    (e) => cb(e.payload)
  );
}

export function onSearchDone(
  cb: (p: { hits: SearchHit[]; filesScanned: number; truncated: boolean }) => void
) {
  return listen<{
    hits: SearchHit[];
    filesScanned: number;
    truncated: boolean;
  }>("search-done", (e) => cb(e.payload));
}

export function onLspDiagnostics(
  cb: (p: { uri: string; diagnostics: LspDiagnostic[] }) => void
) {
  return listen<{ uri: string; diagnostics: LspDiagnostic[] }>(
    "lsp-diagnostics",
    (e) => cb(e.payload)
  );
}
