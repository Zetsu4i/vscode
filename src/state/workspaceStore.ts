import { create } from "zustand";
import { ipc, FileEntry } from "../ipc";
import { baseName } from "../util/paths";
import { useGitStore } from "./gitStore";

interface WorkspaceState {
  root: string | null;
  rootName: string;
  /** dir path -> direct children */
  tree: Record<string, FileEntry[]>;
  expanded: Record<string, boolean>;
  recentFolders: string[];

  openFolder: (root: string) => Promise<void>;
  closeFolder: () => void;
  loadDir: (dir: string, force?: boolean) => Promise<void>;
  toggleDir: (dir: string) => Promise<void>;
  refreshAll: () => Promise<void>;
  initFromSaved: () => Promise<void>;
}

const LS_KEY = "vstauri.workspace";
const LS_RECENT = "vstauri.recentFolders";

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  root: null,
  rootName: "",
  tree: {},
  expanded: {},
  recentFolders: JSON.parse(localStorage.getItem(LS_RECENT) ?? "[]"),

  openFolder: async (root) => {
    // normalize trailing separators
    const norm = root.replace(/[\\/]+$/, "");
    const expanded = { [norm]: true };
    set({
      root: norm,
      rootName: baseName(norm),
      tree: {},
      expanded,
    });
    localStorage.setItem(LS_KEY, JSON.stringify(norm));

    // Widen (or re-scope) the vstauri:// asset sandbox to this workspace root.
    try {
      await ipc.setAssetRoots([norm]);
    } catch (e) {
      console.error("asset roots failed", e);
    }

    const recents = [norm, ...get().recentFolders.filter((r) => r !== norm)].slice(0, 8);
    set({ recentFolders: recents });
    localStorage.setItem(LS_RECENT, JSON.stringify(recents));

    try {
      await ipc.watchFolder(norm);
    } catch (e) {
      console.error("watch failed", e);
    }
    await get().loadDir(norm, true);
    useGitStore.getState().refresh();
  },

  closeFolder: () => {
    set({ root: null, rootName: "", tree: {}, expanded: {} });
    localStorage.removeItem(LS_KEY);
    // Close the asset sandbox: no workspace, no servable files.
    void ipc.setAssetRoots([]).catch(() => {});
    useGitStore.getState().reset();
  },

  loadDir: async (dir, force = false) => {
    if (!force && get().tree[dir]) return;
    try {
      const entries = await ipc.listDir(dir);
      set((s) => ({ tree: { ...s.tree, [dir]: entries } }));
    } catch (e) {
      console.error("listDir failed", dir, e);
    }
  },

  toggleDir: async (dir) => {
    const { expanded } = get();
    if (expanded[dir]) {
      set((s) => ({ expanded: { ...s.expanded, [dir]: false } }));
    } else {
      set((s) => ({ expanded: { ...s.expanded, [dir]: true } }));
      await get().loadDir(dir);
    }
  },

  refreshAll: async () => {
    const { root, expanded } = get();
    if (!root) return;
    const dirs = [root, ...Object.keys(expanded).filter((k) => expanded[k])];
    await Promise.all(dirs.map((d) => get().loadDir(d, true)));
  },

  initFromSaved: async () => {
    const saved = localStorage.getItem(LS_KEY);
    if (saved) {
      try {
        const root = JSON.parse(saved);
        if (typeof root === "string") {
          const { ipc: i } = await import("../ipc");
          const exists = await i.fileExists(root);
          if (exists) {
            await get().openFolder(root);
          }
        }
      } catch {
        localStorage.removeItem(LS_KEY);
      }
    }
  },
}));
