import { create } from "zustand";
import { applyTheme, getStoredThemeId } from "../theme/themes";

export type SidebarView = "explorer" | "search" | "git" | "extensions";

// ---- layout persistence (restore-before-show: applied on first render) ----

const LS_LAYOUT = "vstauri.layout";

interface PersistedLayout {
  view: SidebarView;
  sidebarVisible: boolean;
  sidebarWidth: number;
  panelVisible: boolean;
  panelHeight: number;
  panelTab: "terminal" | "problems";
}

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

function loadLayout(): Partial<PersistedLayout> {
  try {
    const raw = localStorage.getItem(LS_LAYOUT);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

const savedLayout = loadLayout();
const savedView: SidebarView = (["explorer", "search", "git", "extensions"] as const).includes(
  savedLayout.view as SidebarView
)
  ? (savedLayout.view as SidebarView)
  : "explorer";

export interface MenuItem {
  label?: string;
  icon?: string;
  action?: () => void;
  danger?: boolean;
  separator?: boolean;
}

export interface InputDialogState {
  title: string;
  value: string;
  placeholder?: string;
  onOk: (value: string) => void;
}

export interface ConfirmDialogState {
  title: string;
  message: string;
  okLabel?: string;
  onOk?: () => void;
}

export interface RevealRequest {
  path: string;
  line: number;
  col: number;
  token: number;
}

interface UiState {
  view: SidebarView;
  themeId: string;
  sidebarVisible: boolean;
  sidebarWidth: number;
  panelVisible: boolean;
  panelHeight: number;
  panelTab: "terminal" | "problems";
  paletteOpen: boolean;
  paletteMode: "files" | "commands" | "themes";
  contextMenu: { x: number; y: number; items: MenuItem[] } | null;
  inputDialog: InputDialogState | null;
  confirmDialog: ConfirmDialogState | null;
  cursor: { line: number; col: number };
  lspStatuses: Record<string, "running" | "stopped" | "unavailable">;
  reveal: RevealRequest | null;

  setView: (v: SidebarView) => void;
  setTheme: (id: string) => void;
  toggleSidebar: () => void;
  setSidebarWidth: (w: number) => void;
  togglePanel: () => void;
  setPanelHeight: (h: number) => void;
  setPanelTab: (t: "terminal" | "problems") => void;
  openPalette: (mode: "files" | "commands" | "themes") => void;
  closePalette: () => void;
  openContextMenu: (x: number, y: number, items: MenuItem[]) => void;
  closeContextMenu: () => void;
  showInput: (s: InputDialogState) => void;
  closeInput: () => void;
  showConfirm: (s: ConfirmDialogState) => void;
  closeConfirm: () => void;
  setCursor: (line: number, col: number) => void;
  setLspStatus: (lang: string, s: "running" | "stopped" | "unavailable") => void;
  requestReveal: (path: string, line: number, col: number) => void;
}

export const useUiStore = create<UiState>((set, get) => ({
  view: savedView,
  themeId: getStoredThemeId(),
  sidebarVisible: savedLayout.sidebarVisible ?? true,
  sidebarWidth: clamp(savedLayout.sidebarWidth ?? 300, 170, 640),
  panelVisible: savedLayout.panelVisible ?? false,
  panelHeight: clamp(savedLayout.panelHeight ?? 280, 120, 1000),
  panelTab: savedLayout.panelTab === "problems" ? "problems" : "terminal",
  paletteOpen: false,
  paletteMode: "files",
  contextMenu: null,
  inputDialog: null,
  confirmDialog: null,
  cursor: { line: 1, col: 1 },
  lspStatuses: {},
  reveal: null,

  setView: (v) => set({ view: v, sidebarVisible: true }),
  setTheme: (id) => {
    applyTheme(id, true);
    set({ themeId: id });
  },
  toggleSidebar: () => set((s) => ({ sidebarVisible: !s.sidebarVisible })),
  setSidebarWidth: (w) => set({ sidebarWidth: Math.min(640, Math.max(170, w)) }),
  togglePanel: () => set((s) => ({ panelVisible: !s.panelVisible })),
  setPanelHeight: (h) => set({ panelHeight: Math.min(window.innerHeight - 200, Math.max(120, h)) }),
  setPanelTab: (t) => set({ panelTab: t, panelVisible: true }),
  openPalette: (mode) => set({ paletteOpen: true, paletteMode: mode }),
  closePalette: () => set({ paletteOpen: false }),
  openContextMenu: (x, y, items) => set({ contextMenu: { x, y, items } }),
  closeContextMenu: () => set({ contextMenu: null }),
  showInput: (s) => set({ inputDialog: s }),
  closeInput: () => set({ inputDialog: null }),
  showConfirm: (s) => set({ confirmDialog: s }),
  closeConfirm: () => set({ confirmDialog: null }),
  setCursor: (line, col) => {
    const cur = get().cursor;
    if (cur.line !== line || cur.col !== col) set({ cursor: { line, col } });
  },
  setLspStatus: (lang, s) =>
    set((st) => ({ lspStatuses: { ...st.lspStatuses, [lang]: s } })),
  requestReveal: (path, line, col) =>
    set({ reveal: { path, line, col, token: Date.now() } }),
}));

// Debounced layout persistence: saved ~250ms after the last layout change,
// applied synchronously on the next startup (before the window is shown).
let layoutTimer: ReturnType<typeof setTimeout> | null = null;
useUiStore.subscribe((s) => {
  if (layoutTimer) clearTimeout(layoutTimer);
  layoutTimer = setTimeout(() => {
    const l: PersistedLayout = {
      view: s.view,
      sidebarVisible: s.sidebarVisible,
      sidebarWidth: s.sidebarWidth,
      panelVisible: s.panelVisible,
      panelHeight: s.panelHeight,
      panelTab: s.panelTab,
    };
    try {
      localStorage.setItem(LS_LAYOUT, JSON.stringify(l));
    } catch {
      /* storage full/unavailable — layout just won't persist */
    }
  }, 250);
});
