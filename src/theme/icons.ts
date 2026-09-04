import { create } from "zustand";
import { useSettingsStore } from "../state/settingsStore";

/**
 * File icon themes. Icons are inline SVG data URIs — no assets, no network,
 * crisp at any DPI. The default "VSTauri Color" theme gives every common
 * file type a compact colored badge; "Minimal" disables icons entirely.
 */

export interface FileIconTheme {
  id: string;
  label: string;
  /** Returns a data-URI image, or null when this theme shows no icon. */
  getIcon: (name: string, isDir: boolean, expanded: boolean) => string | null;
}

// ---- svg helpers ---------------------------------------------------------------

function svgUri(svg: string): string {
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function badge(text: string, color: string): string {
  const size = text.length > 2 ? 6.5 : text.length > 1 ? 7.5 : 9.5;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><text x="8" y="11.6" text-anchor="middle" font-family="Menlo,Consolas,monospace" font-size="${size}" font-weight="700" fill="${color}">${text}</text></svg>`;
  return svgUri(svg);
}

function folder(expanded: boolean): string {
  const fill = expanded ? "#c99d5e" : "#dcb67a";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M1.5 3.5h4.2l1.4 1.8h7.4v7.2h-13z" fill="${fill}"/></svg>`;
  return svgUri(svg);
}

function fileOutline(): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M4 1.5h5.2L12.5 4.8v9.7h-8.5z" fill="none" stroke="#909090" stroke-width="1.1"/><path d="M9 1.8v3.2h3.2" fill="none" stroke="#909090" stroke-width="1.1"/></svg>`;
  return svgUri(svg);
}

function image(): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1" fill="none" stroke="#a074c4" stroke-width="1.2"/><circle cx="5.6" cy="6.4" r="1.3" fill="#a074c4"/><path d="M3.5 12l3.4-3.6 2.3 2.3 1.9-2 2.4 3.3z" fill="#a074c4"/></svg>`;
  return svgUri(svg);
}

function gitBranch(): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.8" fill="#e8734a"/><circle cx="4.5" cy="12.5" r="1.8" fill="#e8734a"/><circle cx="11.5" cy="5.5" r="1.8" fill="#e8734a"/><path d="M4.5 5.3v5.4M11.5 7.3c0 2.2-2.4 2.6-5.2 3" fill="none" stroke="#e8734a" stroke-width="1.2"/></svg>`;
  return svgUri(svg);
}

function lock(): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="3.5" y="7" width="9" height="6.5" rx="1" fill="#9b9b9b"/><path d="M5.5 7V5a2.5 2.5 0 015 0v2" fill="none" stroke="#9b9b9b" stroke-width="1.4"/></svg>`;
  return svgUri(svg);
}

// ---- ext/name table --------------------------------------------------------------

/** ext (without dot) → [label, color] */
const EXT_ICONS: Record<string, [string, string]> = {
  ts: ["TS", "#3178c6"],
  tsx: ["TS", "#3178c6"],
  mts: ["TS", "#3178c6"],
  cts: ["TS", "#3178c6"],
  js: ["JS", "#f1e05a"],
  jsx: ["JS", "#f1e05a"],
  mjs: ["JS", "#f1e05a"],
  cjs: ["JS", "#f1e05a"],
  json: ["{}", "#cbcb41"],
  jsonc: ["{}", "#cbcb41"],
  rs: ["Rs", "#dea584"],
  py: ["Py", "#3572a5"],
  md: ["Md", "#519aba"],
  markdown: ["Md", "#519aba"],
  html: ["<>", "#e44d26"],
  htm: ["<>", "#e44d26"],
  xml: ["<>", "#87aa3c"],
  css: ["#", "#519aba"],
  scss: ["#", "#c6538c"],
  sass: ["#", "#c6538c"],
  less: ["#", "#2b5e91"],
  toml: ["Tm", "#9c8f6b"],
  yaml: ["Ym", "#a074c4"],
  yml: ["Ym", "#a074c4"],
  sh: [">_", "#89e051"],
  bash: [">_", "#89e051"],
  zsh: [">_", "#89e051"],
  fish: [">_", "#89e051"],
  ps1: [">_", "#89e051"],
  c: ["C", "#5f8faf"],
  h: ["H", "#5f8faf"],
  cpp: ["C+", "#f34b7d"],
  cc: ["C+", "#f34b7d"],
  cxx: ["C+", "#f34b7d"],
  hpp: ["C+", "#f34b7d"],
  hxx: ["C+", "#f34b7d"],
  cs: ["C#", "#178600"],
  go: ["Go", "#00add8"],
  java: ["Jv", "#b07219"],
  php: ["Ph", "#4f5d95"],
  rb: ["Rb", "#701516"],
  lua: ["Lu", "#000080"],
  swift: ["Sw", "#f05138"],
  kt: ["Kt", "#a97bff"],
  sql: ["Sq", "#dd7c3c"],
  vue: ["Vu", "#41b883"],
  svelte: ["Sv", "#ff3e00"],
  txt: ["Tx", "#b5b5b5"],
  log: ["Lg", "#8a8a8a"],
  pdf: ["Pd", "#d93831"],
  zip: ["Zp", "#ac8b53"],
  tar: ["Zp", "#ac8b53"],
  gz: ["Zp", "#ac8b53"],
  wasm: ["Wa", "#654ff0"],
  proto: ["Pt", "#4a8af4"],
  graphql: ["Gq", "#e535ab"],
  gql: ["Gq", "#e535ab"],
};

const IMAGE_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "svg", "bmp", "ico", "webp", "avif",
]);

const NAME_ICONS: Record<string, string> = {
  "dockerfile": badge("Dk", "#2496ed"),
  "makefile": badge("Mk", "#8a8a8a"),
  "cmakelists.txt": badge("Cm", "#8a8a8a"),
  "license": badge("§", "#d4b639"),
  "license.md": badge("§", "#d4b639"),
  "readme.md": badge("Rd", "#519aba"),
  ".gitignore": gitBranch(),
  ".gitattributes": gitBranch(),
  ".editorconfig": badge("Ec", "#9b9b9b"),
};

function colorIcon(name: string, isDir: boolean, expanded: boolean): string | null {
  if (isDir) return folder(expanded);
  const lower = name.toLowerCase();
  if (NAME_ICONS[lower]) return NAME_ICONS[lower];
  if (lower.endsWith(".lock")) return lock();
  const dot = lower.lastIndexOf(".");
  if (dot >= 0) {
    const ext = lower.slice(dot + 1);
    if (IMAGE_EXTS.has(ext)) return image();
    const hit = EXT_ICONS[ext];
    if (hit) return badge(hit[0], hit[1]);
  }
  return fileOutline();
}

// ---- themes -----------------------------------------------------------------------

export const VSTAURI_COLOR: FileIconTheme = {
  id: "vstauri-color",
  label: "VSTauri Color",
  getIcon: colorIcon,
};

export const MINIMAL: FileIconTheme = {
  id: "minimal",
  label: "Minimal (none)",
  getIcon: () => null,
};

interface IconThemeState {
  themes: FileIconTheme[];
  activeId: string;
}

export const useIconThemes = create<IconThemeState>(() => ({
  themes: [VSTAURI_COLOR, MINIMAL],
  activeId: "vstauri-color",
}));

export function getIconTheme(): FileIconTheme {
  const { themes, activeId } = useIconThemes.getState();
  return themes.find((t) => t.id === activeId) ?? VSTAURI_COLOR;
}

/** Convenience for React components: current theme object (reactive). */
export function useIconTheme(): FileIconTheme {
  return useIconThemes((s) => s.themes.find((t) => t.id === s.activeId) ?? VSTAURI_COLOR);
}

let initialized = false;

/** Follow workbench.iconTheme from settings. */
export function initIconThemes(): void {
  if (initialized) return;
  initialized = true;
  void import("../state/settingsStore").then(({ useSettingsStore }) => {
    const apply = (): void => {
      const id = useSettingsStore.getState().iconTheme;
      useIconThemes.setState({ activeId: id });
    };
    apply();
    useSettingsStore.subscribe(apply);
  });
}
