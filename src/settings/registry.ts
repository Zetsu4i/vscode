import { THEMES } from "../theme/themes";

/**
 * Declarative settings registry — VSCode-compatible setting IDs and
 * defaults. The Settings UI renders from this; appliers read through
 * `settingsStore.get(key)` which resolves workspace > user > default.
 */
export interface SettingDef {
  key: string;
  title: string;
  description: string;
  category: "Text Editor" | "Files" | "Terminal" | "Workbench";
  type: "boolean" | "number" | "string" | "enum";
  default: unknown;
  options?: { value: string; label: string }[];
  min?: number;
  max?: number;
}

export const SETTING_CATEGORIES: SettingDef["category"][] = [
  "Text Editor",
  "Files",
  "Terminal",
  "Workbench",
];

const MONO_FONT = 'Consolas, "Courier New", "Droid Sans Mono", monospace';

export const SETTINGS: SettingDef[] = [
  // ---- Files ----
  {
    key: "files.autoSave",
    title: "Auto Save",
    description:
      "Controls auto save of editors that have unsaved changes. afterDelay saves a dirty editor after the configured delay.",
    category: "Files",
    type: "enum",
    default: "off",
    options: [
      { value: "off", label: "off" },
      { value: "afterDelay", label: "afterDelay" },
    ],
  },
  {
    key: "files.autoSaveDelay",
    title: "Auto Save Delay",
    description:
      "Controls the delay in milliseconds after which a dirty editor is saved automatically (used by files.autoSave: afterDelay).",
    category: "Files",
    type: "number",
    default: 1000,
    min: 1,
    max: 600000,
  },
  // ---- Text Editor ----
  {
    key: "editor.fontSize",
    title: "Font Size",
    description: "Controls the font size in pixels.",
    category: "Text Editor",
    type: "number",
    default: 14,
    min: 6,
    max: 48,
  },
  {
    key: "editor.fontFamily",
    title: "Font Family",
    description: "Controls the font family.",
    category: "Text Editor",
    type: "string",
    default: MONO_FONT,
  },
  {
    key: "editor.fontLigatures",
    title: "Font Ligatures",
    description: "Enables or disables font ligatures (==, =>, !==, ...).",
    category: "Text Editor",
    type: "boolean",
    default: false,
  },
  {
    key: "editor.wordWrap",
    title: "Word Wrap",
    description: "Controls how lines should wrap.",
    category: "Text Editor",
    type: "boolean",
    default: false,
  },
  {
    key: "editor.tabSize",
    title: "Tab Size",
    description: "The number of spaces a tab is equal to.",
    category: "Text Editor",
    type: "number",
    default: 4,
    min: 1,
    max: 16,
  },
  {
    key: "editor.lineNumbers",
    title: "Line Numbers",
    description: "Controls the display of line numbers.",
    category: "Text Editor",
    type: "enum",
    default: "on",
    options: [
      { value: "on", label: "on" },
      { value: "off", label: "off" },
    ],
  },
  {
    key: "editor.minimap.enabled",
    title: "Minimap: Enabled",
    description: "Controls whether the minimap is shown.",
    category: "Text Editor",
    type: "boolean",
    default: true,
  },
  // ---- Terminal ----
  {
    key: "terminal.integrated.fontSize",
    title: "Terminal > Integrated: Font Size",
    description: "Controls the font size in pixels of the terminal.",
    category: "Terminal",
    type: "number",
    default: 14,
    min: 6,
    max: 48,
  },
  {
    key: "terminal.integrated.fontFamily",
    title: "Terminal > Integrated: Font Family",
    description: "Controls the font family of the terminal.",
    category: "Terminal",
    type: "string",
    default: MONO_FONT,
  },
  // ---- Workbench ----
  {
    key: "workbench.colorTheme",
    title: "Color Theme",
    description: "Specifies the color theme used in the workbench.",
    category: "Workbench",
    type: "enum",
    default: "dark-modern",
    options: THEMES.map((t) => ({ value: t.id, label: t.label })),
  },
];

const REGISTRY = new Map(SETTINGS.map((d) => [d.key, d]));

export function settingDef(key: string): SettingDef | undefined {
  return REGISTRY.get(key);
}

export function settingDefault<T>(key: string): T {
  return REGISTRY.get(key)?.default as T;
}
