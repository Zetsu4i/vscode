const EXT_LANG: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  jsonc: "json",
  md: "markdown",
  markdown: "markdown",
  css: "css",
  scss: "scss",
  less: "less",
  html: "html",
  htm: "html",
  xml: "xml",
  svg: "xml",
  yml: "yaml",
  yaml: "yaml",
  py: "python",
  pyi: "python",
  pyw: "python",
  rs: "rust",
  go: "go",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  cs: "csharp",
  java: "java",
  rb: "ruby",
  php: "php",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  sql: "sql",
  lua: "lua",
  pl: "perl",
  pm: "perl",
  swift: "swift",
  kt: "kotlin",
  kts: "kotlin",
  bat: "bat",
  cmd: "bat",
  ps1: "powershell",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  toml: "ini",
  properties: "ini",
  diff: "diff",
  patch: "diff",
  dockerfile: "dockerfile",
  graphql: "graphql",
  gql: "graphql",
  proto: "proto",
  hcl: "hcl",
  tf: "hcl",
  vue: "html",
  dart: "dart",
};

const FILE_LANG: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "shell",
  cmakelists: "cmake",
  ".gitignore": "ini",
  ".gitattributes": "ini",
  ".editorconfig": "ini",
  ".env": "ini",
  cargo: "toml",
};

export function baseName(p: string): string {
  const parts = p.split(/[\\/]/);
  return parts[parts.length - 1] || p;
}

export function dirName(p: string): string {
  const idx = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return idx < 0 ? "" : p.slice(0, idx);
}

export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  const sep = dir.includes("\\") ? "\\" : "/";
  return dir.endsWith("/") || dir.endsWith("\\")
    ? dir + name
    : dir + sep + name;
}

export function relativePath(root: string, p: string): string {
  if (root && p.startsWith(root)) {
    const rest = p.slice(root.length);
    return rest.startsWith("/") || rest.startsWith("\\") ? rest.slice(1) : rest;
  }
  return p;
}

export function extOf(p: string): string {
  const b = baseName(p).toLowerCase();
  const idx = b.lastIndexOf(".");
  return idx <= 0 ? "" : b.slice(idx + 1);
}

/** Monaco language id for a path ("" = plain text). */
export function languageForPath(p: string): string {
  const lower = baseName(p).toLowerCase();
  if (FILE_LANG[lower]) return FILE_LANG[lower];
  if (lower.endsWith(".dockerfile") || lower === "dockerfile") return "dockerfile";
  return EXT_LANG[extOf(p)] ?? "";
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
