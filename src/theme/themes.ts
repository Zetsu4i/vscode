import * as monaco from "monaco-editor";

/**
 * VSTauri theme registry.
 *
 * Every theme is a full workbench theme: chrome colors (applied as CSS
 * custom properties on :root), Monaco token theme, and an xterm palette.
 * Values are taken from the real microsoft/vscode theme definitions
 * (theme-defaults / theme-monokai extensions) so each theme is
 * pixel-faithful to the original.
 */

export type ThemeKind = "dark" | "light";

export interface XtermPalette {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export interface WorkbenchTheme {
  id: string;
  label: string;
  kind: ThemeKind;
  /** CSS custom properties applied to document.documentElement */
  ui: Record<string, string>;
  monaco: monaco.editor.IStandaloneThemeData;
  xterm: XtermPalette;
}

/** Dark+ terminal ANSI palette (shared by both dark defaults themes). */
const XTERM_DARK: XtermPalette = {
  background: "#1e1e1e",
  foreground: "#cccccc",
  cursor: "#ffffff",
  cursorAccent: "#1e1e1e",
  selectionBackground: "rgba(255,255,255,0.25)",
  black: "#000000",
  red: "#cd3131",
  green: "#0dbc79",
  yellow: "#e5e510",
  blue: "#2472c8",
  magenta: "#bc3fbc",
  cyan: "#11a8cd",
  white: "#e5e5e5",
  brightBlack: "#666666",
  brightRed: "#f14c4c",
  brightGreen: "#23d18b",
  brightYellow: "#f5f543",
  brightBlue: "#3b8eea",
  brightMagenta: "#d670d6",
  brightCyan: "#29b8db",
  brightWhite: "#ffffff",
};

/** Light themes terminal ANSI palette (theme-defaults light). */
const XTERM_LIGHT: XtermPalette = {
  background: "#ffffff",
  foreground: "#3b3b3b",
  cursor: "#333333",
  cursorAccent: "#ffffff",
  selectionBackground: "rgba(0,0,0,0.25)",
  black: "#000000",
  red: "#cd3131",
  green: "#00bc00",
  yellow: "#949800",
  blue: "#0451a5",
  magenta: "#bc05bc",
  cyan: "#0598bc",
  white: "#555555",
  brightBlack: "#666666",
  brightRed: "#cd3131",
  brightGreen: "#14ce14",
  brightYellow: "#b5ba00",
  brightBlue: "#0451a5",
  brightMagenta: "#bc05bc",
  brightCyan: "#0598bc",
  brightWhite: "#a5a5a5",
};

/** Monaco token rules shared by Dark+ and Dark Modern (dark_vs chain). */
const DARK_TOKEN_RULES: monaco.editor.ITokenThemeRule[] = [
  { token: "", foreground: "d4d4d4" },
  { token: "comment", foreground: "6a9955", fontStyle: "italic" },
  { token: "keyword", foreground: "569cd6" },
  { token: "keyword.flow", foreground: "c586c0" },
  { token: "number", foreground: "b5cea8" },
  { token: "number.hex", foreground: "b5cea8" },
  { token: "string", foreground: "ce9178" },
  { token: "string.escape", foreground: "d7ba7d" },
  { token: "regexp", foreground: "d16969" },
  { token: "type", foreground: "4ec9b0" },
  { token: "type.identifier", foreground: "4ec9b0" },
  { token: "identifier", foreground: "9cdcfe" },
  { token: "function", foreground: "dcdcaa" },
  { token: "delimiter", foreground: "d4d4d4" },
  { token: "delimiter.bracket", foreground: "ffd700" },
  { token: "operator", foreground: "d4d4d4" },
  { token: "tag", foreground: "569cd6" },
  { token: "metatag", foreground: "569cd6" },
  { token: "attribute.name", foreground: "9cdcfe" },
  { token: "attribute.value", foreground: "ce9178" },
  { token: "variable", foreground: "9cdcfe" },
  { token: "variable.predefined", foreground: "4fc1ff" },
  { token: "constant", foreground: "4fc1ff" },
  { token: "namespace", foreground: "c586c0" },
  { token: "predefined", foreground: "4ec9b0" },
  { token: "annotation", foreground: "dcdcaa" },
];

/** Monaco token rules for the light defaults themes (light_vs chain). */
const LIGHT_TOKEN_RULES: monaco.editor.ITokenThemeRule[] = [
  { token: "", foreground: "000000" },
  { token: "comment", foreground: "008000" },
  { token: "keyword", foreground: "0000ff" },
  { token: "keyword.flow", foreground: "af00db" },
  { token: "number", foreground: "098658" },
  { token: "string", foreground: "a31515" },
  { token: "string.escape", foreground: "a31515" },
  { token: "regexp", foreground: "811f3f" },
  { token: "type", foreground: "267f99" },
  { token: "type.identifier", foreground: "267f99" },
  { token: "identifier", foreground: "001080" },
  { token: "function", foreground: "795e26" },
  { token: "delimiter", foreground: "000000" },
  { token: "operator", foreground: "000000" },
  { token: "tag", foreground: "800000" },
  { token: "metatag", foreground: "800000" },
  { token: "attribute.name", foreground: "e50000" },
  { token: "attribute.value", foreground: "0451a5" },
  { token: "variable", foreground: "001080" },
  { token: "constant", foreground: "0070c1" },
  { token: "annotation", foreground: "795e26" },
];

/* ------------------------------------------------------------------ */
/* Dark+ (default dark) — the classic look                            */
/* ------------------------------------------------------------------ */

const DARK_PLUS_UI: Record<string, string> = {
  "bg-editor": "#1e1e1e",
  "bg-side": "#252526",
  "bg-activity": "#333333",
  "bg-title": "#3c3c3c",
  "bg-tab": "#2d2d2d",
  "bg-tab-active": "#1e1e1e",
  "bg-panel": "#1e1e1e",
  "bg-input": "#3c3c3c",
  "bg-dropdown": "#454545",
  "bg-hover": "#2a2d2e",
  "bg-list-active": "#37373d",
  "bg-widget": "#252526",
  "bg-button": "#0e639c",
  "bg-button-hover": "#1177bb",
  "bg-button-secondary": "#3a3d41",
  "bg-button-secondary-hover": "#45494e",
  "status-bg": "#007acc",
  "status-fg": "#ffffff",
  "border": "#2b2b2b",
  "border-strong": "#454545",
  "text": "#cccccc",
  "text-strong": "#e7e7e7",
  "text-muted": "#a0a0a0",
  "text-dim": "#868686",
  "accent": "#007fd4",
  "focus": "#007fd4",
  "list-focus": "#062f4a",
  "list-focus-fg": "#ffffff",
  "list-active-fg": "#ffffff",
  "badge-bg": "#4d4d4d",
  "badge-fg": "#ffffff",
  "tab-fg": "#ffffff",
  "tab-inactive-fg": "#969696",
  "titlebar-border": "#333333",
  "quick-bg": "#252526",
  "menu-selection": "#094771",
  "menu-selection-fg": "#ffffff",
  "input-border": "#3c3c3c",
  "hover-weak": "rgba(255,255,255,0.08)",
  "hover-strong": "rgba(255,255,255,0.10)",
  "activity-inactive": "rgba(255,255,255,0.40)",
  "activity-fg": "#ffffff",
  "error": "#f14c4c",
  "warning": "#cca700",
  "link": "#3794ff",
  "scrollbar": "rgba(121,121,121,0.4)",
  "scrollbar-hover": "rgba(100,100,100,0.7)",
  "git-modified": "#e2c08d",
  "git-added": "#81b88b",
  "git-deleted": "#c74e39",
  "git-untracked": "#73c991",
  "git-renamed": "#a09a7f",
};

export const DARK_PLUS: WorkbenchTheme = {
  id: "dark-plus",
  label: "Dark+ (default dark)",
  kind: "dark",
  ui: DARK_PLUS_UI,
  monaco: {
    base: "vs-dark",
    inherit: true,
    rules: DARK_TOKEN_RULES,
    colors: {
      "editor.background": "#1e1e1e",
      "editor.foreground": "#d4d4d4",
      "editorLineNumber.foreground": "#858585",
      "editorLineNumber.activeForeground": "#c6c6c6",
      "editor.selectionBackground": "#264f78",
      "editor.inactiveSelectionBackground": "#3a3d41",
      "editor.lineHighlightBackground": "#282828",
      "editorCursor.foreground": "#aeafad",
      "editorIndentGuide.background1": "#404040",
      "editorIndentGuide.activeBackground1": "#707070",
      "editorWhitespace.foreground": "#3b3b3b",
      "editorBracketMatch.border": "#888888",
      "editorBracketMatch.background": "#1e1e1e00",
      "editorWidget.background": "#252526",
      "editorWidget.border": "#454545",
      "editorSuggestWidget.background": "#252526",
      "editorSuggestWidget.border": "#454545",
      "editorSuggestWidget.selectedBackground": "#04395e",
      "editorHoverWidget.background": "#252526",
      "editorHoverWidget.border": "#454545",
      "editorGutter.background": "#1e1e1e",
      "editorError.foreground": "#f14c4c",
      "editorWarning.foreground": "#cca700",
      "minimap.background": "#1e1e1e",
      "scrollbarSlider.background": "#79797966",
      "scrollbarSlider.hoverBackground": "#646464b3",
      "scrollbarSlider.activeBackground": "#bfbfbf66",
      "input.background": "#3c3c3c",
      "input.border": "#3c3c3c",
      "focusBorder": "#007fd4",
    },
  },
  xterm: XTERM_DARK,
};

/* ------------------------------------------------------------------ */
/* Dark Modern — the modern default                                   */
/* ------------------------------------------------------------------ */

export const DARK_MODERN: WorkbenchTheme = {
  id: "dark-modern",
  label: "Dark Modern",
  kind: "dark",
  ui: {
    "bg-editor": "#1f1f1f",
    "bg-side": "#181818",
    "bg-activity": "#181818",
    "bg-title": "#181818",
    "bg-tab": "#181818",
    "bg-tab-active": "#1f1f1f",
    "bg-panel": "#181818",
    "bg-input": "#313131",
    "bg-dropdown": "#1f1f1f",
    "bg-hover": "#2a2d2e",
    "bg-list-active": "#04395e",
    "bg-widget": "#202020",
    "bg-button": "#0078d4",
    "bg-button-hover": "#026ec1",
    "bg-button-secondary": "#313131",
    "bg-button-secondary-hover": "#3c3c3c",
    "status-bg": "#181818",
    "status-fg": "#cccccc",
    "border": "#2b2b2b",
    "border-strong": "#454545",
    "text": "#cccccc",
    "text-strong": "#ffffff",
    "text-muted": "#9d9d9d",
    "text-dim": "#868686",
    "accent": "#0078d4",
    "focus": "#0078d4",
    "list-focus": "#04395e",
    "list-focus-fg": "#ffffff",
    "list-active-fg": "#ffffff",
    "badge-bg": "#616161",
    "badge-fg": "#f8f8f8",
    "tab-fg": "#ffffff",
    "tab-inactive-fg": "#9d9d9d",
    "titlebar-border": "#2b2b2b",
    "quick-bg": "#222222",
    "menu-selection": "#0078d4",
    "menu-selection-fg": "#ffffff",
    "input-border": "#3c3c3c",
    "hover-weak": "rgba(255,255,255,0.08)",
    "hover-strong": "rgba(241,241,241,0.20)",
    "activity-inactive": "#868686",
    "activity-fg": "#d7d7d7",
    "error": "#f14c4c",
    "warning": "#cca700",
    "link": "#4daafc",
    "scrollbar": "rgba(121,121,121,0.4)",
    "scrollbar-hover": "rgba(100,100,100,0.7)",
    "git-modified": "#e2c08d",
    "git-added": "#81b88b",
    "git-deleted": "#c74e39",
    "git-untracked": "#73c991",
    "git-renamed": "#a09a7f",
  },
  monaco: {
    base: "vs-dark",
    inherit: true,
    rules: DARK_TOKEN_RULES,
    colors: {
      "editor.background": "#1f1f1f",
      "editor.foreground": "#cccccc",
      "editorLineNumber.foreground": "#6e7681",
      "editorLineNumber.activeForeground": "#cccccc",
      "editor.selectionBackground": "#264f78",
      "editor.inactiveSelectionBackground": "#3a3d41",
      "editor.lineHighlightBackground": "#282828",
      "editorCursor.foreground": "#aeafad",
      "editorIndentGuide.background1": "#404040",
      "editorIndentGuide.activeBackground1": "#707070",
      "editorWhitespace.foreground": "#3b3b3b",
      "editorBracketMatch.border": "#888888",
      "editorBracketMatch.background": "#1e1e1e00",
      "editorWidget.background": "#202020",
      "editorWidget.border": "#454545",
      "editorSuggestWidget.background": "#202020",
      "editorSuggestWidget.border": "#454545",
      "editorSuggestWidget.selectedBackground": "#04395e",
      "editorHoverWidget.background": "#202020",
      "editorHoverWidget.border": "#454545",
      "editorGutter.background": "#1f1f1f",
      "editorError.foreground": "#f14c4c",
      "editorWarning.foreground": "#cca700",
      "minimap.background": "#1f1f1f",
      "scrollbarSlider.background": "#79797966",
      "scrollbarSlider.hoverBackground": "#646464b3",
      "scrollbarSlider.activeBackground": "#bfbfbf66",
      "input.background": "#313131",
      "input.border": "#3c3c3c",
      "focusBorder": "#0078d4",
    },
  },
  xterm: { ...XTERM_DARK, background: "#1f1f1f", foreground: "#cccccc" },
};

/* ------------------------------------------------------------------ */
/* Light+ (default light)                                             */
/* ------------------------------------------------------------------ */

const LIGHT_PLUS_UI: Record<string, string> = {
  "bg-editor": "#ffffff",
  "bg-side": "#f3f3f3",
  "bg-activity": "#2c2c2c",
  "bg-title": "#dddddd",
  "bg-tab": "#ececec",
  "bg-tab-active": "#ffffff",
  "bg-panel": "#ffffff",
  "bg-input": "#ffffff",
  "bg-dropdown": "#ffffff",
  "bg-hover": "#e8e8e8",
  "bg-list-active": "#e4e6f1",
  "bg-widget": "#f3f3f3",
  "bg-button": "#007acc",
  "bg-button-hover": "#006bb3",
  "bg-button-secondary": "#e0e0e0",
  "bg-button-secondary-hover": "#d4d4d4",
  "status-bg": "#007acc",
  "status-fg": "#ffffff",
  "border": "#c8c8c8",
  "border-strong": "#cecece",
  "text": "#3b3b3b",
  "text-strong": "#333333",
  "text-muted": "#616161",
  "text-dim": "#767676",
  "accent": "#0090fb",
  "focus": "#0090fb",
  "list-focus": "#0060c0",
  "list-focus-fg": "#ffffff",
  "list-active-fg": "#1f1f1f",
  "badge-bg": "#c4c4c4",
  "badge-fg": "#333333",
  "tab-fg": "#333333",
  "tab-inactive-fg": "#6f6f6f",
  "titlebar-border": "#c8c8c8",
  "quick-bg": "#f3f3f3",
  "menu-selection": "#0060c0",
  "menu-selection-fg": "#ffffff",
  "input-border": "#cecece",
  "hover-weak": "rgba(0,0,0,0.06)",
  "hover-strong": "rgba(0,0,0,0.10)",
  "activity-inactive": "rgba(255,255,255,0.40)",
  "activity-fg": "#ffffff",
  "error": "#e51400",
  "warning": "#bf8803",
  "link": "#006ab1",
  "scrollbar": "rgba(100,100,100,0.4)",
  "scrollbar-hover": "rgba(100,100,100,0.7)",
  "git-modified": "#895503",
  "git-added": "#388a34",
  "git-deleted": "#ad0707",
  "git-untracked": "#388a34",
  "git-renamed": "#665900",
};

export const LIGHT_PLUS: WorkbenchTheme = {
  id: "light-plus",
  label: "Light+ (default light)",
  kind: "light",
  ui: LIGHT_PLUS_UI,
  monaco: {
    base: "vs",
    inherit: true,
    rules: LIGHT_TOKEN_RULES,
    colors: {
      "editor.background": "#ffffff",
      "editor.foreground": "#000000",
      "editorLineNumber.foreground": "#237893",
      "editorLineNumber.activeForeground": "#0b216f",
      "editor.selectionBackground": "#add6ff",
      "editor.inactiveSelectionBackground": "#e5ebf1",
      "editor.lineHighlightBackground": "#f2f2f2",
      "editorCursor.foreground": "#000000",
      "editorIndentGuide.background1": "#d3d3d3",
      "editorIndentGuide.activeBackground1": "#939393",
      "editorWhitespace.foreground": "#d3d3d3",
      "editorBracketMatch.border": "#007acc",
      "editorBracketMatch.background": "#00000000",
      "editorWidget.background": "#f3f3f3",
      "editorWidget.border": "#c8c8c8",
      "editorSuggestWidget.background": "#f3f3f3",
      "editorSuggestWidget.border": "#c8c8c8",
      "editorSuggestWidget.selectedBackground": "#0060c0",
      "editorHoverWidget.background": "#f3f3f3",
      "editorHoverWidget.border": "#c8c8c8",
      "editorGutter.background": "#ffffff",
      "editorError.foreground": "#e51400",
      "editorWarning.foreground": "#bf8803",
      "minimap.background": "#ffffff",
      "scrollbarSlider.background": "#64646433",
      "scrollbarSlider.hoverBackground": "#64646459",
      "scrollbarSlider.activeBackground": "#64646480",
      "input.background": "#ffffff",
      "input.border": "#cecece",
      "focusBorder": "#0090fb",
    },
  },
  xterm: XTERM_LIGHT,
};

/* ------------------------------------------------------------------ */
/* Light Modern                                                       */
/* ------------------------------------------------------------------ */

export const LIGHT_MODERN: WorkbenchTheme = {
  id: "light-modern",
  label: "Light Modern",
  kind: "light",
  ui: {
    ...LIGHT_PLUS_UI,
    "bg-side": "#f8f8f8",
    "bg-activity": "#f8f8f8",
    "bg-title": "#f8f8f8",
    "bg-tab": "#f8f8f8",
    "bg-tab-active": "#ffffff",
    "bg-panel": "#f8f8f8",
    "bg-widget": "#f8f8f8",
    "bg-button": "#005fb8",
    "bg-button-hover": "#0258a8",
    "bg-button-secondary": "#e5e5e5",
    "bg-button-secondary-hover": "#cccccc",
    "status-bg": "#f8f8f8",
    "status-fg": "#3b3b3b",
    "border": "#e5e5e5",
    "border-strong": "#cecece",
    "text-strong": "#1e1e1e",
    "accent": "#005fb8",
    "focus": "#005fb8",
    "list-focus": "#e8e8e8",
    "list-focus-fg": "#000000",
    "badge-bg": "#cccccc",
    "badge-fg": "#3b3b3b",
    "tab-fg": "#3b3b3b",
    "tab-inactive-fg": "#868686",
    "titlebar-border": "#e5e5e5",
    "quick-bg": "#f8f8f8",
    "menu-selection": "#005fb8",
    "menu-selection-fg": "#ffffff",
    "input-border": "#cecece",
    "hover-weak": "rgba(31,31,31,0.07)",
    "hover-strong": "rgba(31,31,31,0.11)",
    "activity-inactive": "#616161",
    "activity-fg": "#1f1f1f",
    "link": "#006ab1",
  },
  monaco: {
    base: "vs",
    inherit: true,
    rules: LIGHT_TOKEN_RULES,
    colors: {
      "editor.background": "#ffffff",
      "editor.foreground": "#3b3b3b",
      "editorLineNumber.foreground": "#6e7681",
      "editorLineNumber.activeForeground": "#171184",
      "editor.selectionBackground": "#add6ff",
      "editor.inactiveSelectionBackground": "#e5ebf1",
      "editor.lineHighlightBackground": "#f5f5f5",
      "editorCursor.foreground": "#000000",
      "editorIndentGuide.background1": "#d3d3d3",
      "editorIndentGuide.activeBackground1": "#939393",
      "editorWhitespace.foreground": "#d3d3d3",
      "editorBracketMatch.border": "#005fb8",
      "editorBracketMatch.background": "#00000000",
      "editorWidget.background": "#f8f8f8",
      "editorWidget.border": "#cecece",
      "editorSuggestWidget.background": "#f8f8f8",
      "editorSuggestWidget.border": "#cecece",
      "editorSuggestWidget.selectedBackground": "#e8e8e8",
      "editorHoverWidget.background": "#f8f8f8",
      "editorHoverWidget.border": "#cecece",
      "editorGutter.background": "#ffffff",
      "editorError.foreground": "#e51400",
      "editorWarning.foreground": "#bf8803",
      "minimap.background": "#ffffff",
      "scrollbarSlider.background": "#64646433",
      "scrollbarSlider.hoverBackground": "#64646459",
      "scrollbarSlider.activeBackground": "#64646480",
      "input.background": "#ffffff",
      "input.border": "#cecece",
      "focusBorder": "#005fb8",
    },
  },
  xterm: { ...XTERM_LIGHT, foreground: "#3b3b3b" },
};

/* ------------------------------------------------------------------ */
/* Monokai                                                            */
/* ------------------------------------------------------------------ */

export const MONOKAI: WorkbenchTheme = {
  id: "monokai",
  label: "Monokai",
  kind: "dark",
  ui: {
    "bg-editor": "#272822",
    "bg-side": "#1e1f1c",
    "bg-activity": "#272822",
    "bg-title": "#1e1f1c",
    "bg-tab": "#34352f",
    "bg-tab-active": "#272822",
    "bg-panel": "#272822",
    "bg-input": "#414339",
    "bg-dropdown": "#1e1f1c",
    "bg-hover": "#3e3d32",
    "bg-list-active": "#414339",
    "bg-widget": "#1e1f1c",
    "bg-button": "#75715e",
    "bg-button-hover": "#837f6d",
    "bg-button-secondary": "#414339",
    "bg-button-secondary-hover": "#4d4f44",
    "status-bg": "#414339",
    "status-fg": "#f8f8f2",
    "border": "#1e1f1c",
    "border-strong": "#414339",
    "text": "#cccccc",
    "text-strong": "#f8f8f2",
    "text-muted": "#a6a69b",
    "text-dim": "#8f8f86",
    "accent": "#99947c",
    "focus": "#99947c",
    "list-focus": "#75715e",
    "list-focus-fg": "#f8f8f2",
    "list-active-fg": "#f8f8f2",
    "badge-bg": "#75715e",
    "badge-fg": "#f8f8f2",
    "tab-fg": "#f8f8f2",
    "tab-inactive-fg": "#a6a69b",
    "titlebar-border": "#1e1f1c",
    "quick-bg": "#1e1f1c",
    "menu-selection": "#75715e",
    "menu-selection-fg": "#f8f8f2",
    "input-border": "#414339",
    "hover-weak": "rgba(248,248,242,0.08)",
    "hover-strong": "rgba(248,248,242,0.12)",
    "activity-inactive": "rgba(248,248,242,0.45)",
    "activity-fg": "#f8f8f2",
    "error": "#f92672",
    "warning": "#e6db74",
    "link": "#66d9ef",
    "scrollbar": "rgba(121,121,121,0.4)",
    "scrollbar-hover": "rgba(100,100,100,0.7)",
    "git-modified": "#e2c08d",
    "git-added": "#a6e22e",
    "git-deleted": "#f92672",
    "git-untracked": "#a6e22e",
    "git-renamed": "#e6db74",
  },
  monaco: {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "", foreground: "f8f8f2" },
      { token: "comment", foreground: "88846f", fontStyle: "italic" },
      { token: "keyword", foreground: "66d9ef", fontStyle: "italic" },
      { token: "keyword.flow", foreground: "f92672" },
      { token: "keyword.operator", foreground: "f92672" },
      { token: "number", foreground: "ae81ff" },
      { token: "number.hex", foreground: "ae81ff" },
      { token: "string", foreground: "e6db74" },
      { token: "string.escape", foreground: "ae81ff" },
      { token: "regexp", foreground: "e6db74" },
      { token: "type", foreground: "66d9ef", fontStyle: "italic" },
      { token: "type.identifier", foreground: "a6e22e" },
      { token: "identifier", foreground: "f8f8f2" },
      { token: "function", foreground: "a6e22e" },
      { token: "delimiter", foreground: "f8f8f2" },
      { token: "delimiter.bracket", foreground: "f8f8f2" },
      { token: "operator", foreground: "f92672" },
      { token: "tag", foreground: "f92672" },
      { token: "metatag", foreground: "f92672" },
      { token: "attribute.name", foreground: "a6e22e" },
      { token: "attribute.value", foreground: "e6db74" },
      { token: "variable", foreground: "f8f8f2" },
      { token: "variable.predefined", foreground: "ae81ff" },
      { token: "constant", foreground: "ae81ff" },
      { token: "namespace", foreground: "a6e22e" },
      { token: "annotation", foreground: "a6e22e" },
    ],
    colors: {
      "editor.background": "#272822",
      "editor.foreground": "#f8f8f2",
      "editorLineNumber.foreground": "#90908a",
      "editorLineNumber.activeForeground": "#c2c2bf",
      "editor.selectionBackground": "#878b9180",
      "editor.inactiveSelectionBackground": "#414339",
      "editor.lineHighlightBackground": "#3e3d32",
      "editorCursor.foreground": "#f8f8f0",
      "editorIndentGuide.background1": "#464741",
      "editorIndentGuide.activeBackground1": "#75715e",
      "editorWhitespace.foreground": "#464741",
      "editorBracketMatch.border": "#75715e",
      "editorBracketMatch.background": "#27282200",
      "editorWidget.background": "#1e1f1c",
      "editorWidget.border": "#414339",
      "editorSuggestWidget.background": "#1e1f1c",
      "editorSuggestWidget.border": "#414339",
      "editorSuggestWidget.selectedBackground": "#414339",
      "editorHoverWidget.background": "#1e1f1c",
      "editorHoverWidget.border": "#414339",
      "editorGutter.background": "#272822",
      "editorError.foreground": "#f92672",
      "editorWarning.foreground": "#e6db74",
      "minimap.background": "#272822",
      "scrollbarSlider.background": "#75715e66",
      "scrollbarSlider.hoverBackground": "#75715e99",
      "scrollbarSlider.activeBackground": "#75715eb3",
      "input.background": "#414339",
      "input.border": "#414339",
      "focusBorder": "#99947c",
    },
  },
  xterm: {
    background: "#272822",
    foreground: "#f8f8f2",
    cursor: "#f8f8f0",
    cursorAccent: "#272822",
    selectionBackground: "rgba(135,139,145,0.50)",
    black: "#333333",
    red: "#c4265e",
    green: "#86b42b",
    yellow: "#b3b42b",
    blue: "#6a7ec8",
    magenta: "#8c6bc8",
    cyan: "#56adbc",
    white: "#e3e3dd",
    brightBlack: "#666666",
    brightRed: "#f92672",
    brightGreen: "#a6e22e",
    brightYellow: "#e2e22e",
    brightBlue: "#819aff",
    brightMagenta: "#ae81ff",
    brightCyan: "#66d9ef",
    brightWhite: "#f8f8f2",
  },
};

/* ------------------------------------------------------------------ */
/* Registry + application                                             */
/* ------------------------------------------------------------------ */

export const THEMES: WorkbenchTheme[] = [
  DARK_MODERN,
  DARK_PLUS,
  LIGHT_MODERN,
  LIGHT_PLUS,
  MONOKAI,
];

const THEME_STORAGE_KEY = "vstauri.colorTheme";

export function getTheme(id: string): WorkbenchTheme {
  return THEMES.find((t) => t.id === id) ?? DARK_MODERN;
}

export function getStoredThemeId(): string {
  try {
    return localStorage.getItem(THEME_STORAGE_KEY) ?? DARK_MODERN.id;
  } catch {
    return DARK_MODERN.id;
  }
}

/** Apply a theme's chrome colors as CSS custom properties on <html>. */
function applyCssVariables(ui: Record<string, string>): void {
  const rootStyle = document.documentElement.style;
  for (const [name, value] of Object.entries(ui)) {
    rootStyle.setProperty(`--${name}`, value);
  }
}

/**
 * Apply a theme everywhere: chrome CSS variables, `data-vscode-kind` on
 * <html> (dark/light), and Monaco. `persist` writes the choice to
 * localStorage (live preview passes false so Escape can revert).
 */
export function applyTheme(id: string, persist = false): WorkbenchTheme {
  const theme = getTheme(id);
  applyCssVariables(theme.ui);
  document.documentElement.dataset.vscodeKind = theme.kind;
  document.documentElement.dataset.vscodeTheme = theme.id;
  monaco.editor.defineTheme(theme.id, theme.monaco);
  monaco.editor.setTheme(theme.id);
  if (persist) {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, theme.id);
    } catch {
      /* private mode — theme just won't persist */
    }
  }
  return theme;
}

