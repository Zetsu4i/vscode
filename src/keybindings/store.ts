import { create } from "zustand";
import { ipc } from "../ipc";
import { commands } from "../commands";

/**
 * Keybinding resolution, VSCode-style:
 * - defaults come from the command registry (`keybinding` fields)
 * - the user's keybindings.json may unbind defaults (rule with command
 *   prefixed "-") and/or assign new keys; later rules win
 */

export interface KbRule {
  key: string;
  command: string;
}

interface KeybindingState {
  userRules: KbRule[];
  loaded: boolean;
  /** command id currently waiting for a new key press (editor UI). */
  captureFor: string | null;

  init: () => Promise<void>;
  setUserRules: (rules: KbRule[]) => Promise<void>;
  /** Assign a new key to a command (null resets to default = removes user rule). */
  rebind: (commandId: string, key: string | null) => Promise<void>;
  setCapture: (commandId: string | null) => void;
}

/** Normalize "Ctrl+Shift+P" → "ctrl+shift+p" with canonical modifier order. */
export function normalizeKey(key: string): string {
  const parts = key
    .toLowerCase()
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  const mods = ["ctrl", "shift", "alt", "meta"];
  const held = mods.filter((m) => parts.includes(m));
  const rest = parts.filter((p) => !mods.includes(p));
  return [...held, ...rest].join("+");
}

/** Build the normalized string for a live KeyboardEvent. */
export function eventKey(e: KeyboardEvent): string {
  let key = e.key.toLowerCase();
  // Shift+` produces "~" on most layouts — treat both as the same key
  // (the hardcoded layer used to special-case them).
  if (e.shiftKey && key === "~") key = "`";
  const held: string[] = [];
  if (e.ctrlKey || e.metaKey) held.push("ctrl");
  if (e.shiftKey) held.push("shift");
  if (e.altKey) held.push("alt");
  return [...held, key].join("+");
}

/** Default bindings derived from the command registry. */
export function defaultRules(): KbRule[] {
  return commands
    .filter((c) => !!c.keybinding)
    .map((c) => ({ key: normalizeKey(c.keybinding!), command: c.id }));
}

/**
 * Resolve the effective binding map (normalized key → command id).
 * Defaults minus removals, then user rules in order (later wins).
 */
export function resolveBindings(userRules: KbRule[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const r of defaultRules()) map.set(r.key, r.command);
  for (const r of userRules) {
    const key = normalizeKey(r.key);
    if (!key) continue;
    if (r.command.startsWith("-")) {
      // unbind: remove matching assignments for that command
      const target = r.command.slice(1);
      for (const [k, cmd] of [...map.entries()]) {
        if (cmd === target && (key === "" || k === key)) map.delete(k);
      }
    } else {
      map.set(key, r.command);
    }
  }
  return map;
}

export const useKeybindingStore = create<KeybindingState>((set, get) => {
  const persist = (rules: KbRule[]): void => {
    void ipc
      .configWrite("user", "keybindings.json", rules)
      .catch((e) => console.error("persist keybindings failed", e));
  };

  return {
    userRules: [],
    loaded: false,
    captureFor: null,

    init: async () => {
      try {
        const doc = await ipc.configRead("user", "keybindings.json");
        if (Array.isArray(doc)) {
          const rules = doc
            .filter(
              (r): r is KbRule =>
                !!r &&
                typeof r === "object" &&
                typeof (r as KbRule).command === "string" &&
                typeof (r as KbRule).key === "string"
            )
            .map((r) => ({ key: r.key, command: r.command }));
          set({ userRules: rules, loaded: true });
          return;
        }
      } catch (e) {
        console.error("read keybindings failed", e);
      }
      set({ loaded: true });
    },

    setUserRules: async (rules) => {
      set({ userRules: rules });
      persist(rules);
    },

    rebind: async (commandId, key) => {
      // drop every user rule that targets this command
      const kept = get().userRules.filter(
        (r) => !r.command.startsWith("-") && r.command !== commandId
      );
      if (key === null) {
        await get().setUserRules(kept);
      } else {
        await get().setUserRules([...kept, { key, command: commandId }]);
      }
    },

    setCapture: (commandId) => set({ captureFor: commandId }),
  };
});

/** Human display form: "ctrl+shift+p" → "Ctrl+Shift+P". */
export function displayKey(normalized: string): string {
  return normalized
    .split("+")
    .map((p) => (p.length === 1 ? p.toUpperCase() : p.charAt(0).toUpperCase() + p.slice(1)))
    .join("+");
}
