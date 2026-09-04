import { create } from "zustand";

/**
 * Editor preferences. Phase 1 keeps them in localStorage; Phase 2 migrates
 * this to a user settings.json with scopes.
 */
interface SettingsState {
  breadcrumbs: boolean;
  stickyScroll: boolean;

  toggleBreadcrumbs: () => void;
  toggleStickyScroll: () => void;
  set: (patch: Partial<Omit<SettingsState, "toggleBreadcrumbs" | "toggleStickyScroll" | "set">>) => void;
}

const KEY = "vstauri.settings.v1";

function load(): Pick<SettingsState, "breadcrumbs" | "stickyScroll"> {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<Pick<SettingsState, "breadcrumbs" | "stickyScroll">>;
      return {
        breadcrumbs: parsed.breadcrumbs ?? true,
        stickyScroll: parsed.stickyScroll ?? true,
      };
    }
  } catch {
    /* corrupted storage → defaults */
  }
  return { breadcrumbs: true, stickyScroll: true };
}

function persist(s: SettingsState): void {
  try {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({ breadcrumbs: s.breadcrumbs, stickyScroll: s.stickyScroll })
    );
  } catch {
    /* storage unavailable — session-only settings */
  }
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  ...load(),

  toggleBreadcrumbs: () => {
    set((s) => ({ breadcrumbs: !s.breadcrumbs }));
    persist(get());
  },
  toggleStickyScroll: () => {
    set((s) => ({ stickyScroll: !s.stickyScroll }));
    persist(get());
  },
  set: (patch) => {
    set(patch);
    persist(get());
  },
}));
