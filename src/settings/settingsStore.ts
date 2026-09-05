import { create } from "zustand";
import { ipc } from "../ipc";
import { useWorkspaceStore } from "../state/workspaceStore";
import { settingDefault } from "./registry";

export type SettingsScope = "user" | "workspace";

interface SettingsState {
  scope: SettingsScope;
  /** dotted-key -> value, parsed from the user settings file */
  userValues: Record<string, unknown>;
  /** dotted-key -> value, parsed from the workspace settings file */
  workspaceValues: Record<string, unknown>;
  userPath: string | null;
  workspacePath: string | null;
  /** bumped on every load/change so appliers can react */
  revision: number;
  loaded: boolean;

  load: () => Promise<void>;
  setScope: (s: SettingsScope) => void;
  /** resolve a setting: workspace > user > registry default */
  get: <T>(key: string) => T;
  /** whether the key is overridden in any scope (or a specific one) */
  hasKey: (key: string, scope?: SettingsScope) => boolean;
  /** which scope currently overrides the key, if any */
  modifiedIn: (key: string) => SettingsScope | null;
  /** write a key into the CURRENT scope's settings file */
  set: (key: string, value: unknown) => Promise<void>;
  /** remove a key from the CURRENT scope (falls back to the default) */
  reset: (key: string) => Promise<void>;
  /** reload when a settings file changed on disk (watcher hook) */
  maybeReload: (changedPaths: string[]) => Promise<void>;
}

/** Flatten nested objects into dotted keys: {"editor":{"fontSize":14}} ->
 *  {"editor.fontSize":14}. Empty objects are kept as leaf values. */
function flatten(obj: Record<string, unknown>, prefix = "", out: Record<string, unknown> = {}) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (
      v !== null &&
      typeof v === "object" &&
      !Array.isArray(v) &&
      Object.keys(v as Record<string, unknown>).length > 0
    ) {
      flatten(v as Record<string, unknown>, key, out);
    } else {
      out[key] = v;
    }
  }
  return out;
}

function parseSettingsFile(text: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(text);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return flatten(parsed as Record<string, unknown>);
    }
  } catch {
    /* malformed file — treat as empty; the UI shows the JSON for fixing */
  }
  return {};
}

function prettyWrite(values: Record<string, unknown>): string {
  const sorted: Record<string, unknown> = {};
  for (const k of Object.keys(values).sort()) sorted[k] = values[k];
  return JSON.stringify(sorted, null, 2) + "\n";
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  scope: "user",
  userValues: {},
  workspaceValues: {},
  userPath: null,
  workspacePath: null,
  revision: 0,
  loaded: false,

  load: async () => {
    const root = useWorkspaceStore.getState().root;
    try {
      const user = await ipc.settingsRead("user", null);
      const workspace = root ? await ipc.settingsRead("workspace", root) : null;
      set((s) => ({
        userValues: parseSettingsFile(user.text),
        workspaceValues: workspace ? parseSettingsFile(workspace.text) : {},
        userPath: user.path,
        workspacePath: workspace?.path ?? null,
        revision: s.revision + 1,
        loaded: true,
      }));
    } catch (e) {
      console.error("settings load failed", e);
      set((s) => ({ loaded: true, revision: s.revision + 1 }));
    }
  },

  setScope: (scope) => set({ scope }),

  get: <T,>(key: string): T => {
    const { userValues, workspaceValues } = get();
    if (key in workspaceValues) return workspaceValues[key] as T;
    if (key in userValues) return userValues[key] as T;
    return settingDefault<T>(key);
  },

  hasKey: (key, scope) => {
    const { userValues, workspaceValues } = get();
    if (scope === "user") return key in userValues;
    if (scope === "workspace") return key in workspaceValues;
    return key in userValues || key in workspaceValues;
  },

  modifiedIn: (key) => {
    const { userValues, workspaceValues } = get();
    if (key in workspaceValues) return "workspace";
    if (key in userValues) return "user";
    return null;
  },

  set: async (key, value) => {
    if (!get().loaded) await get().load();
    const scope = get().scope;
    const root = useWorkspaceStore.getState().root;
    if (scope === "workspace" && !root) return;

    const current =
      scope === "user" ? { ...get().userValues } : { ...get().workspaceValues };
    current[key] = value;

    try {
      await ipc.settingsWrite(scope, scope === "workspace" ? root : null, prettyWrite(current));
    } catch (e) {
      console.error("settings write failed", e);
      return;
    }
    set((s) => ({
      userValues: scope === "user" ? current : s.userValues,
      workspaceValues: scope === "workspace" ? current : s.workspaceValues,
      revision: s.revision + 1,
    }));
  },

  reset: async (key) => {
    if (!get().loaded) await get().load();
    const scope = get().scope;
    const root = useWorkspaceStore.getState().root;
    if (scope === "workspace" && !root) return;

    const current =
      scope === "user" ? { ...get().userValues } : { ...get().workspaceValues };
    if (!(key in current)) return;
    delete current[key];

    try {
      await ipc.settingsWrite(scope, scope === "workspace" ? root : null, prettyWrite(current));
    } catch (e) {
      console.error("settings reset failed", e);
      return;
    }
    set((s) => ({
      userValues: scope === "user" ? current : s.userValues,
      workspaceValues: scope === "workspace" ? current : s.workspaceValues,
      revision: s.revision + 1,
    }));
  },

  maybeReload: async (changedPaths) => {
    const { userPath, workspacePath } = get();
    const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();
    const hits = changedPaths.filter(
      (p) =>
        (userPath && norm(p) === norm(userPath)) ||
        (workspacePath && norm(p) === norm(workspacePath))
    );
    if (hits.length > 0) await get().load();
  },
}));
