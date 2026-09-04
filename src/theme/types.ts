import type * as monaco from "monaco-editor";

/** xterm.js color palette (subset ITheme we control). */
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

/**
 * A complete workbench theme: workbench chrome (CSS custom properties),
 * editor (Monaco theme data) and terminal (xterm palette) — one object
 * drives all three surfaces, the way a VSCode theme does.
 */
export interface WorkbenchTheme {
  id: string;
  label: string;
  kind: "dark" | "light";
  /** CSS custom properties applied to documentElement (camel-free, --names). */
  css: Record<string, string>;
  monaco: monaco.editor.IStandaloneThemeData;
  xterm: XtermPalette;
  /** Set when the theme comes from an extension contribution. */
  extensionId?: string;
}
