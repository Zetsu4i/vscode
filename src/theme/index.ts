import { create } from "zustand";
import { joinPath } from "../util/paths";
import { DARK_PLUS } from "./darkplus";
import { LIGHT_PLUS } from "./lightplus";
import { MONOKAI } from "./monokai";
import type { WorkbenchTheme } from "./types";

// ---- registry ---------------------------------------------------------------

interface ThemeRegistryState {
  themes: WorkbenchTheme[];
  /** Register (or replace) a theme; id is the key. */
  register: (theme: WorkbenchTheme) => void;
}

export const useThemeRegistry = create<ThemeRegistryState>((set) => ({
  themes: [],
  register: (theme) =>
    set((s) => ({
      themes: [...s.themes.filter((t) => t.id !== theme.id), theme],
    })),
}));

export function listThemeOptions(): { value: string; label: string }[] {
  return useThemeRegistry.getState().themes.map((t) => ({ value: t.id, label: t.label }));
}

// ---- application --------------------------------------------------------------

let activeId = "dark-plus";
let applied = false;
const themeListeners = new Set<(t: WorkbenchTheme) => void>();
let initialized = false;

export function getActiveTheme(): WorkbenchTheme {
  return useThemeRegistry.getState().themes.find((t) => t.id === activeId) ?? DARK_PLUS;
}

/** Subscribe to applied-theme changes (used by the terminal for xterm). */
export function onThemeChange(cb: (t: WorkbenchTheme) => void): () => void {
  themeListeners.add(cb);
  if (applied) cb(getActiveTheme());
  return () => themeListeners.delete(cb);
}

function applyTheme(id: string): void {
  const theme = useThemeRegistry.getState().themes.find((t) => t.id === id) ?? DARK_PLUS;
  activeId = theme.id;

  const root = document.documentElement;
  for (const [k, v] of Object.entries(theme.css)) {
    root.style.setProperty(k, v);
  }
  root.style.colorScheme = theme.kind;
  document.body.classList.toggle("theme-light", theme.kind === "light");

  // Monaco theme is registered lazily right before use.
  void import("../monaco").then(({ monaco }) => {
    monaco.editor.defineTheme(theme.id, theme.monaco);
    monaco.editor.setTheme(theme.id);
  });

  for (const cb of themeListeners) cb(theme);
}

// ---- extension theme conversion -----------------------------------------------

/** Map VSCode workbench color keys → our CSS variables. */
const COLOR_TO_VAR: Record<string, string> = {
  "editor.background": "--bg-editor",
  "sideBar.background": "--bg-side",
  "activityBar.background": "--bg-activity",
  "titleBar.activeBackground": "--bg-title",
  "tab.inactiveBackground": "--bg-tab",
  "tab.activeBackground": "--bg-tab-active",
  "panel.background": "--bg-panel",
  "input.background": "--bg-input",
  "dropdown.background": "--bg-dropdown",
  "list.hoverBackground": "--bg-hover",
  "list.activeSelectionBackground": "--bg-list-active",
  "list.activeSelectionForeground": "--list-active-fg",
  "editorWidget.background": "--bg-widget",
  "button.background": "--bg-button",
  "statusBar.background": "--status-bg",
  "statusBar.noFolderBackground": "--status-bg",
  "statusBar.foreground": "--status-fg",
  "panel.border": "--border",
  "editorGroup.border": "--border",
  "foreground": "--text",
  "descriptionForeground": "--text-muted",
  "titleBar.activeForeground": "--text-strong",
  "sideBarTitle.foreground": "--text-muted",
  "focusBorder": "--focus",
  "errorForeground": "--error",
  "editorWarning.foreground": "--warning",
  "textLink.foreground": "--link",
  "gitDecoration.modifiedResourceForeground": "--git-modified",
  "gitDecoration.addedResourceForeground": "--git-added",
  "gitDecoration.deletedResourceForeground": "--git-deleted",
  "gitDecoration.untrackedResourceForeground": "--git-untracked",
  "gitDecoration.renamedResourceForeground": "--git-renamed",
};

/** Heuristic TextMate scope → Monaco token mapping. */
function scopeToToken(scope: string): string | null {
  const s = scope.toLowerCase();
  if (s.includes("comment")) return "comment";
  if (s.includes("string")) return "string";
  if (s.includes("keyword.control")) return "keyword.flow";
  if (s.includes("keyword") || s.includes("storage")) return "keyword";
  if (s.includes("constant.numeric")) return "number";
  if (s.includes("variable.predefined") || s.includes("support.constant")) return "variable.predefined";
  if (s.includes("constant")) return "constant";
  if (s.includes("entity.name.type") || s.includes("support.type") || s.includes("entity.other.inherited")) return "type";
  if (s.includes("entity.name.function") || s.includes("support.function")) return "function";
  if (s.includes("entity.name.tag")) return "tag";
  if (s.includes("entity.other.attribute-name")) return "attribute.name";
  if (s.includes("variable")) return "variable";
  if (s.includes("punctuation")) return "delimiter";
  if (s.includes("meta.tag")) return "tag";
  return null;
}

interface TokenColor {
  scope?: string | string[];
  settings?: { foreground?: string; fontStyle?: string };
}

/** Convert an extension's JSON theme file into a WorkbenchTheme. */
function convertThemeJson(
  id: string,
  label: string,
  extKind: string,
  doc: Record<string, unknown>,
  extensionId: string
): WorkbenchTheme | null {
  const fileKind = typeof doc.type === "string" ? doc.type : extKind;
  const kind: "dark" | "light" = fileKind === "light" ? "light" : "dark";
  const colors =
    doc.colors && typeof doc.colors === "object"
      ? (doc.colors as Record<string, string>)
      : {};
  const tokenColors = Array.isArray(doc.tokenColors)
    ? (doc.tokenColors as TokenColor[])
    : [];

  const css: Record<string, string> = {};
  for (const [key, value] of Object.entries(colors)) {
    const v = COLOR_TO_VAR[key];
    if (v && typeof value === "string") css[v] = value;
  }

  const rules: { token: string; foreground?: string; fontStyle?: string }[] = [];
  const seen = new Set<string>();
  for (const tc of tokenColors) {
    const fg = tc.settings?.foreground;
    const style = tc.settings?.fontStyle;
    const scopes = Array.isArray(tc.scope) ? tc.scope : tc.scope ? [tc.scope] : [""];
    for (const scope of scopes) {
      const token = scope ? scopeToToken(scope) : "";
      if (token === null || seen.has(token)) continue;
      seen.add(token);
      const rule: { token: string; foreground?: string; fontStyle?: string } = { token };
      if (fg && typeof fg === "string") rule.foreground = fg.replace("#", "").slice(0, 6);
      if (style) rule.fontStyle = style;
      rules.push(rule);
    }
  }

  // Monaco colors: keep recognized editor.* keys as-is.
  const monacoColors: Record<string, string> = {};
  for (const [key, value] of Object.entries(colors)) {
    if (typeof value === "string" && key.startsWith("editor.")) monacoColors[key] = value;
  }
  if (!monacoColors["editor.background"] && css["--bg-editor"]) {
    monacoColors["editor.background"] = css["--bg-editor"];
  }

  return {
    id,
    label,
    kind,
    css,
    extensionId,
    monaco: {
      base: kind === "light" ? "vs" : "vs-dark",
      inherit: true,
      rules,
      colors: monacoColors,
    },
    xterm: {
      background: css["--bg-editor"] ?? (kind === "light" ? "#ffffff" : "#1e1e1e"),
      foreground: css["--text"] ?? (kind === "light" ? "#3b3b3b" : "#cccccc"),
      cursor: kind === "light" ? "#000000" : "#ffffff",
      cursorAccent: css["--bg-editor"] ?? (kind === "light" ? "#ffffff" : "#1e1e1e"),
      selectionBackground: kind === "light" ? "rgba(0,0,0,0.25)" : "rgba(255,255,255,0.25)",
      black: "#000000",
      red: "#cd3131",
      green: kind === "light" ? "#00bc00" : "#0dbc79",
      yellow: kind === "light" ? "#949800" : "#e5e510",
      blue: kind === "light" ? "#0451a5" : "#2472c8",
      magenta: "#bc05bc",
      cyan: kind === "light" ? "#0598bc" : "#11a8cd",
      white: "#555555",
      brightBlack: "#666666",
      brightRed: "#f14c4c",
      brightGreen: kind === "light" ? "#23d18b" : "#23d18b",
      brightYellow: "#f5f543",
      brightBlue: "#3b8eea",
      brightMagenta: "#d670d6",
      brightCyan: "#29b8db",
      brightWhite: "#ffffff",
    },
  };
}

async function registerExtensionThemes(): Promise<void> {
  try {
    const { ipc } = await import("../ipc");
    const { useWorkspaceStore } = await import("../state/workspaceStore");
    const root = useWorkspaceStore.getState().root ?? undefined;
    const exts = await ipc.listExtensions(root);
    for (const ext of exts) {
      for (const contrib of ext.manifest.contributes.themes ?? []) {
        if (!contrib.path) continue;
        try {
          const fc = await ipc.readFile(joinPath(ext.dir, contrib.path));
          const doc = JSON.parse(fc.content) as Record<string, unknown>;
          const theme = convertThemeJson(
            `ext:${ext.manifest.id}:${contrib.path}`,
            contrib.label || ext.manifest.name,
            contrib.kind,
            doc,
            ext.manifest.id
          );
          if (theme) useThemeRegistry.getState().register(theme);
        } catch (e) {
          console.error(`theme load failed: ${ext.manifest.id}/${contrib.path}`, e);
        }
      }
    }
  } catch {
    /* extensions are optional — built-in themes always exist */
  }
}

// ---- init ---------------------------------------------------------------------

/** Register built-ins, apply the configured theme, follow future changes. */
export function initThemes(): void {
  if (initialized) return;
  initialized = true;

  useThemeRegistry.getState().register(DARK_PLUS);
  useThemeRegistry.getState().register(LIGHT_PLUS);
  useThemeRegistry.getState().register(MONOKAI);

  // Follow the configured setting (dark-plus until settings finish loading).
  const applyFromSettings = (themeId: unknown): void => {
    applyTheme(typeof themeId === "string" ? themeId : "dark-plus");
  };
  void import("../state/settingsStore").then(({ useSettingsStore }) => {
    applyFromSettings(useSettingsStore.getState().colorTheme);
    useSettingsStore.subscribe((s) => applyFromSettings(s.colorTheme));
  });

  void registerExtensionThemes();
}

export type { WorkbenchTheme };
