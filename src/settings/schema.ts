/**
 * Settings schema — the single source of truth describing every setting:
 * its id, type, default, which scope it can live in, and how the Settings
 * editor presents it. Ids follow VSCode conventions so that a user's
 * mental model (and muscle memory) transfers directly.
 */

export type SettingType = "boolean" | "number" | "string" | "enum";

export interface SettingEnumOption {
  value: string;
  label: string;
}

export interface SettingDef {
  id: string;
  type: SettingType;
  default: unknown;
  description: string;
  category: "Editor" | "Workbench" | "Files";
  /** user = global only; resource = overridable per workspace. */
  scope: "user" | "resource";
  enumValues?: SettingEnumOption[];
  numeric?: { min: number; max: number; step?: number };
}

export const SETTINGS: SettingDef[] = [
  // ---- Workbench -----------------------------------------------------------
  {
    id: "workbench.colorTheme",
    type: "enum",
    default: "dark-plus",
    description: "Specifies the color theme used in the workbench.",
    category: "Workbench",
    scope: "user",
    enumValues: [
      { value: "dark-plus", label: "Dark+ (default dark)" },
      { value: "light-plus", label: "Light+ (default light)" },
      { value: "monokai", label: "Monokai" },
    ],
  },
  {
    id: "workbench.iconTheme",
    type: "enum",
    default: "vstauri-color",
    description: "Specifies the file icon theme used in the workbench.",
    category: "Workbench",
    scope: "user",
    enumValues: [
      { value: "vstauri-color", label: "VSTauri Color" },
      { value: "minimal", label: "Minimal (none)" },
    ],
  },
  // ---- Editor --------------------------------------------------------------
  {
    id: "editor.fontSize",
    type: "number",
    default: 14,
    description: "Controls the font size in pixels.",
    category: "Editor",
    scope: "resource",
    numeric: { min: 8, max: 40, step: 1 },
  },
  {
    id: "editor.fontFamily",
    type: "string",
    default: 'Consolas, "Courier New", "Droid Sans Mono", monospace',
    description: "Controls the font family used by the editor.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "editor.fontLigatures",
    type: "boolean",
    default: false,
    description: "Enables font ligatures in the editor.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "editor.tabSize",
    type: "number",
    default: 4,
    description: "The number of spaces a tab is equal to.",
    category: "Editor",
    scope: "resource",
    numeric: { min: 1, max: 8, step: 1 },
  },
  {
    id: "editor.wordWrap",
    type: "boolean",
    default: false,
    description: "Controls how lines should wrap.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "editor.minimap.enabled",
    type: "boolean",
    default: true,
    description: "Controls whether the minimap is shown.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "editor.renderWhitespace",
    type: "enum",
    default: "selection",
    description: "Controls how whitespace characters are rendered.",
    category: "Editor",
    scope: "resource",
    enumValues: [
      { value: "none", label: "none" },
      { value: "selection", label: "selection" },
      { value: "all", label: "all" },
    ],
  },
  {
    id: "editor.stickyScroll.enabled",
    type: "boolean",
    default: true,
    description: "Shows the nested current scopes at the top of the editor.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "breadcrumbs.enabled",
    type: "boolean",
    default: true,
    description: "Enable/disable navigation breadcrumbs.",
    category: "Editor",
    scope: "resource",
  },
  {
    id: "editor.formatOnSave",
    type: "boolean",
    default: false,
    description: "Format a file on save, using the language server when one is running.",
    category: "Editor",
    scope: "resource",
  },
  // ---- Files ---------------------------------------------------------------
  {
    id: "files.autoSave",
    type: "enum",
    default: "off",
    description: "Controls auto save of editors that have unsaved changes.",
    category: "Files",
    scope: "resource",
    enumValues: [
      { value: "off", label: "off" },
      { value: "afterDelay", label: "afterDelay" },
    ],
  },
  {
    id: "files.autoSaveDelay",
    type: "number",
    default: 1,
    description: "Controls the delay in seconds after which a dirty editor is saved automatically.",
    category: "Files",
    scope: "resource",
    numeric: { min: 1, max: 60, step: 1 },
  },
];

export const SETTINGS_BY_ID: ReadonlyMap<string, SettingDef> = new Map(
  SETTINGS.map((s) => [s.id, s])
);

export function settingsForCategory(): string[] {
  return [...new Set(SETTINGS.map((s) => s.category))];
}

// ---- dotted-path helpers over nested JSON ------------------------------------

export function getAt(obj: Record<string, unknown>, path: string): unknown {
  let cur: unknown = obj;
  for (const seg of path.split(".")) {
    if (cur === null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[seg];
  }
  return cur;
}

/** Returns a new object with `path` set to `value` (structural sharing). */
export function setAt(
  obj: Record<string, unknown>,
  path: string,
  value: unknown
): Record<string, unknown> {
  const segs = path.split(".");
  const walk = (node: Record<string, unknown>, i: number): Record<string, unknown> => {
    const child = { ...node };
    if (i === segs.length - 1) {
      child[segs[i]] = value;
    } else {
      const next = node[segs[i]];
      child[segs[i]] =
        next !== null && typeof next === "object" && !Array.isArray(next)
          ? walk(next as Record<string, unknown>, i + 1)
          : walk({}, i + 1);
    }
    return child;
  };
  return walk(obj, 0);
}

/** Coerce + validate a raw value from JSON against a setting definition. */
export function coerce(def: SettingDef, raw: unknown): { ok: boolean; value: unknown } {
  switch (def.type) {
    case "boolean":
      return typeof raw === "boolean" ? { ok: true, value: raw } : { ok: false, value: def.default };
    case "number":
      if (typeof raw === "number" && Number.isFinite(raw)) {
        const { min, max } = def.numeric ?? { min: -1e9, max: 1e9 };
        return { ok: true, value: Math.min(max, Math.max(min, raw)) };
      }
      return { ok: false, value: def.default };
    case "enum":
      return def.enumValues?.some((o) => o.value === raw)
        ? { ok: true, value: raw }
        : { ok: false, value: def.default };
    case "string":
      return typeof raw === "string" ? { ok: true, value: raw } : { ok: false, value: def.default };
  }
}
