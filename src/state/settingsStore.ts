import { create } from "zustand";
import { ipc } from "../ipc";
import {
  coerce,
  getAt,
  removeAt,
  setAt,
  SETTINGS,
  SETTINGS_BY_ID,
  type SettingDef,
} from "../settings/schema";
import { useWorkspaceStore } from "./workspaceStore";

export type WhitespaceMode = "selection" | "all" | "none";
export type AutoSaveMode = "off" | "afterDelay";

interface SettingsData {
  // workbench
  colorTheme: string;
  iconTheme: string;
  // editor
  breadcrumbs: boolean;
  stickyScroll: boolean;
  fontSize: number;
  fontFamily: string;
  ligatures: boolean;
  minimap: boolean;
  wordWrap: boolean;
  renderWhitespace: WhitespaceMode;
  tabSize: number;
  formatOnSave: boolean;
  // files
  autoSave: AutoSaveMode;
  autoSaveDelay: number;
}

interface SettingsState extends SettingsData {
  /** True once user + workspace settings.json have been loaded and applied. */
  loaded: boolean;
  /** Raw scope documents (nested JSON, dotted ids). */
  userValues: Record<string, unknown>;
  workspaceValues: Record<string, unknown>;

  init: () => Promise<void>;
  /** Re-read `<root>/.vstauri/settings.json` (called when a folder opens/closes). */
  reloadWorkspaceScope: () => Promise<void>;
  /**
   * Update one setting. `scope` selects where it is persisted; "resource"
   * settings fall back to user scope when no workspace folder is open.
   */
  update: (id: string, value: unknown, scope?: "user" | "workspace") => Promise<void>;
  /** Remove an override from the scope document it lives in (falls back to user). */
  reset: (id: string) => Promise<void>;

  // convenience actions (palette / keybindings / status bar)
  toggleBreadcrumbs: () => void;
  toggleStickyScroll: () => void;
  setFontSize: (px: number) => void;
  increaseFontSize: () => void;
  decreaseFontSize: () => void;
  resetFontSize: () => void;
  toggleLigatures: () => void;
  toggleMinimap: () => void;
  toggleWordWrap: () => void;
  cycleRenderWhitespace: () => void;
  setTabSize: (n: number) => void;
}

const DEFAULTS: SettingsData = {
  colorTheme: "dark-plus",
  iconTheme: "vstauri-color",
  breadcrumbs: true,
  stickyScroll: true,
  fontSize: 14,
  fontFamily: 'Consolas, "Courier New", "Droid Sans Mono", monospace',
  ligatures: false,
  minimap: true,
  wordWrap: false,
  renderWhitespace: "selection",
  tabSize: 4,
  formatOnSave: false,
  autoSave: "off",
  autoSaveDelay: 1,
};

/** schema id → store field */
export const FIELD_OF: Record<string, keyof SettingsData> = {
  "workbench.colorTheme": "colorTheme",
  "workbench.iconTheme": "iconTheme",
  "breadcrumbs.enabled": "breadcrumbs",
  "editor.stickyScroll.enabled": "stickyScroll",
  "editor.fontSize": "fontSize",
  "editor.fontFamily": "fontFamily",
  "editor.fontLigatures": "ligatures",
  "editor.minimap.enabled": "minimap",
  "editor.wordWrap": "wordWrap",
  "editor.renderWhitespace": "renderWhitespace",
  "editor.tabSize": "tabSize",
  "editor.formatOnSave": "formatOnSave",
  "files.autoSave": "autoSave",
  "files.autoSaveDelay": "autoSaveDelay",
};

/** Legacy localStorage store → settings.json ids (one-time migration). */
const LEGACY_FIELD_OF: Partial<Record<keyof SettingsData, string>> = {
  breadcrumbs: "breadcrumbs.enabled",
  stickyScroll: "editor.stickyScroll.enabled",
  fontSize: "editor.fontSize",
  ligatures: "editor.fontLigatures",
  minimap: "editor.minimap.enabled",
  wordWrap: "editor.wordWrap",
  renderWhitespace: "editor.renderWhitespace",
  tabSize: "editor.tabSize",
};

const LS_LEGACY_KEY = "vstauri.settings.v1";

function effective(
  user: Record<string, unknown>,
  workspace: Record<string, unknown>
): SettingsData {
  const out: SettingsData = { ...DEFAULTS };
  for (const def of SETTINGS) {
    const raw =
      getAt(workspace, def.id) !== undefined ? getAt(workspace, def.id) : getAt(user, def.id);
    if (raw !== undefined) {
      const { ok, value } = coerce(def, raw);
      const field = FIELD_OF[def.id];
      if (ok && field) (out as unknown as Record<string, unknown>)[field] = value;
    }
  }
  return out;
}

export const useSettingsStore = create<SettingsState>((set, get) => {
  /** Current workspace root (static import is safe: no cycle back here). */
  const wsRoot = (): string | null => useWorkspaceStore.getState().root;

  /** Write one scope doc to disk (fire-and-forget with error log). */
  const persist = (scope: "user" | "workspace"): void => {
    const root = wsRoot();
    if (scope === "workspace" && !root) return;
    const doc = scope === "user" ? get().userValues : get().workspaceValues;
    void ipc
      .configWrite(scope, "settings.json", doc, root ?? undefined)
      .catch((e) => console.error(`persist ${scope} settings failed`, e));
  };

  const apply = (): void => {
    set((s) => ({ ...effective(s.userValues, s.workspaceValues), loaded: true }));
  };

  /** Re-read `<root>/.vstauri/settings.json`; clears it when no folder is open. */
  const loadWorkspaceScope = async (): Promise<void> => {
    const root = wsRoot();
    if (!root) {
      set({ workspaceValues: {} });
      return;
    }
    try {
      const doc = await ipc.configRead("workspace", "settings.json", root);
      if (doc && typeof doc === "object" && !Array.isArray(doc)) {
        set({ workspaceValues: doc as Record<string, unknown> });
      } else {
        set({ workspaceValues: {} });
      }
    } catch {
      /* unreadable workspace settings — keep defaults */
      set({ workspaceValues: {} });
    }
  };

  const resetScoped = async (id: string): Promise<void> => {
    // remove from the scope document that actually overrides it
    let target: "user" | "workspace" | null = null;
    if (getAt(get().workspaceValues, id) !== undefined) target = "workspace";
    else if (getAt(get().userValues, id) !== undefined) target = "user";
    if (!target) return;
    set((s) =>
      target === "workspace"
        ? { workspaceValues: removeAt(s.workspaceValues, id) }
        : { userValues: removeAt(s.userValues, id) }
    );
    persist(target);
    apply();
  };

  const updateScoped = async (
    id: string,
    value: unknown,
    scope: "user" | "workspace"
  ): Promise<void> => {
    const def = SETTINGS_BY_ID.get(id);
    if (!def) return;
    const { ok, value: v } = coerce(def, value);
    if (!ok) return;

    // Workspace scope requires an open folder and a resource-scoped setting.
    let target = scope;
    if (target === "workspace") {
      const root = wsRoot();
      if (!root || def.scope !== "resource") target = "user";
    }

    set((s) =>
      target === "user"
        ? { userValues: setAt(s.userValues, id, v) }
        : { workspaceValues: setAt(s.workspaceValues, id, v) }
    );
    persist(target);
    apply();
  };

  return {
    ...DEFAULTS,
    loaded: false,
    userValues: {},
    workspaceValues: {},

    init: async () => {
      // one-time migration from the Phase 1 localStorage store
      let migrated: Record<string, unknown> | null = null;
      try {
        const raw = window.localStorage.getItem(LS_LEGACY_KEY);
        if (raw) {
          const legacy = JSON.parse(raw) as Record<string, unknown>;
          migrated = {};
          for (const [field, id] of Object.entries(LEGACY_FIELD_OF)) {
            if (legacy[field] !== undefined) {
              const def = SETTINGS_BY_ID.get(id);
              if (def) {
                migrated = setAt(migrated, id, coerce(def, legacy[field]).value);
              }
            }
          }
        }
      } catch {
        /* corrupted legacy store — ignore */
      }

      let user: Record<string, unknown> = {};
      try {
        const doc = await ipc.configRead("user", "settings.json");
        if (doc && typeof doc === "object" && !Array.isArray(doc)) {
          user = doc as Record<string, unknown>;
        }
      } catch (e) {
        console.error("read user settings failed", e);
      }
      if (migrated && Object.keys(migrated).length > 0) {
        user = { ...migrated, ...user }; // file wins over migrated legacy values
        try {
          window.localStorage.removeItem(LS_LEGACY_KEY);
        } catch {
          /* ignore */
        }
      }
      set({ userValues: user });
      await loadWorkspaceScope();
      apply();
    },

    reloadWorkspaceScope: async () => {
      await loadWorkspaceScope();
      apply();
    },

    update: async (id, value, scope = "user") => {
      await updateScoped(id, value, scope);
    },

    reset: async (id) => {
      await resetScoped(id);
    },

    toggleBreadcrumbs: () => void updateScoped("breadcrumbs.enabled", !get().breadcrumbs, "user"),
    toggleStickyScroll: () =>
      void updateScoped("editor.stickyScroll.enabled", !get().stickyScroll, "user"),
    setFontSize: (px) => {
      const clamped = Math.min(40, Math.max(8, Math.round(px)));
      void updateScoped("editor.fontSize", clamped, "user");
    },
    increaseFontSize: () => get().setFontSize(get().fontSize + 1),
    decreaseFontSize: () => get().setFontSize(get().fontSize - 1),
    resetFontSize: () => get().setFontSize(DEFAULTS.fontSize),
    toggleLigatures: () => void updateScoped("editor.fontLigatures", !get().ligatures, "user"),
    toggleMinimap: () => void updateScoped("editor.minimap.enabled", !get().minimap, "user"),
    toggleWordWrap: () => void updateScoped("editor.wordWrap", !get().wordWrap, "user"),
    cycleRenderWhitespace: () => {
      const order: WhitespaceMode[] = ["selection", "all", "none"];
      const next = order[(order.indexOf(get().renderWhitespace) + 1) % order.length];
      void updateScoped("editor.renderWhitespace", next, "user");
    },
    setTabSize: (n) => {
      const clamped = Math.min(8, Math.max(1, Math.round(n)));
      void updateScoped("editor.tabSize", clamped, "user");
    },
  };
});

export type { SettingDef };
