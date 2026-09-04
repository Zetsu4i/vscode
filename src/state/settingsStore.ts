import { create } from "zustand";

export type WhitespaceMode = "selection" | "all" | "none";

interface SettingsData {
  breadcrumbs: boolean;
  stickyScroll: boolean;
  fontSize: number;
  ligatures: boolean;
  minimap: boolean;
  wordWrap: boolean;
  renderWhitespace: WhitespaceMode;
  tabSize: number;
}

/**
 * Editor appearance & behavior preferences. Phase 1 persists them to
 * localStorage; Phase 2 migrates this to a user settings.json with scopes.
 */
interface SettingsState extends SettingsData {
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

const KEY = "vstauri.settings.v1";
const DEFAULT_FONT = 14;

type Patch = Partial<SettingsData>;

function load(): SettingsData {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Patch;
      return {
        breadcrumbs: parsed.breadcrumbs ?? true,
        stickyScroll: parsed.stickyScroll ?? true,
        fontSize: parsed.fontSize ?? DEFAULT_FONT,
        ligatures: parsed.ligatures ?? false,
        minimap: parsed.minimap ?? true,
        wordWrap: parsed.wordWrap ?? false,
        renderWhitespace: parsed.renderWhitespace ?? "selection",
        tabSize: parsed.tabSize ?? 4,
      };
    }
  } catch {
    /* corrupted storage → defaults */
  }
  return {
    breadcrumbs: true,
    stickyScroll: true,
    fontSize: DEFAULT_FONT,
    ligatures: false,
    minimap: true,
    wordWrap: false,
    renderWhitespace: "selection",
    tabSize: 4,
  };
}

function persist(s: SettingsState): void {
  try {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({
        breadcrumbs: s.breadcrumbs,
        stickyScroll: s.stickyScroll,
        fontSize: s.fontSize,
        ligatures: s.ligatures,
        minimap: s.minimap,
        wordWrap: s.wordWrap,
        renderWhitespace: s.renderWhitespace,
        tabSize: s.tabSize,
      })
    );
  } catch {
    /* storage unavailable — session-only settings */
  }
}

const WHITESPACE_ORDER: WhitespaceMode[] = ["selection", "all", "none"];

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
  setFontSize: (px) => {
    set(() => ({ fontSize: Math.min(40, Math.max(8, Math.round(px))) }));
    persist(get());
  },
  increaseFontSize: () => get().setFontSize(get().fontSize + 1),
  decreaseFontSize: () => get().setFontSize(get().fontSize - 1),
  resetFontSize: () => get().setFontSize(DEFAULT_FONT),
  toggleLigatures: () => {
    set((s) => ({ ligatures: !s.ligatures }));
    persist(get());
  },
  toggleMinimap: () => {
    set((s) => ({ minimap: !s.minimap }));
    persist(get());
  },
  toggleWordWrap: () => {
    set((s) => ({ wordWrap: !s.wordWrap }));
    persist(get());
  },
  cycleRenderWhitespace: () => {
    set((s) => ({
      renderWhitespace:
        WHITESPACE_ORDER[(WHITESPACE_ORDER.indexOf(s.renderWhitespace) + 1) % WHITESPACE_ORDER.length],
    }));
    persist(get());
  },
  setTabSize: (n) => {
    set(() => ({ tabSize: Math.min(8, Math.max(1, Math.round(n))) }));
    persist(get());
  },
}));
