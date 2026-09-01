import { create } from "zustand";
import { ipc, GitChange } from "../ipc";
import { useWorkspaceStore } from "./workspaceStore";

interface GitState {
  repo: boolean;
  branch: string | null;
  changes: GitChange[];
  refreshing: boolean;
  refresh: () => Promise<void>;
  stage: (paths: string[]) => Promise<void>;
  unstage: (paths: string[]) => Promise<void>;
  commit: (message: string) => Promise<void>;
  reset: () => void;
}

export const useGitStore = create<GitState>((set, get) => ({
  repo: false,
  branch: null,
  changes: [],
  refreshing: false,

  refresh: async () => {
    const root = useWorkspaceStore.getState().root;
    if (!root) {
      set({ repo: false, branch: null, changes: [] });
      return;
    }
    set({ refreshing: true });
    try {
      const isRepo = await ipc.gitIsRepo(root);
      if (!isRepo) {
        set({ repo: false, branch: null, changes: [] });
        return;
      }
      const status = await ipc.gitStatus(root);
      set({ repo: true, branch: status.branch, changes: status.changes });
    } catch (e) {
      console.error("git status failed", e);
    } finally {
      set({ refreshing: false });
    }
  },

  stage: async (paths) => {
    const root = useWorkspaceStore.getState().root;
    if (!root) return;
    await ipc.gitStage(root, paths);
    await get().refresh();
  },

  unstage: async (paths) => {
    const root = useWorkspaceStore.getState().root;
    if (!root) return;
    await ipc.gitUnstage(root, paths);
    await get().refresh();
  },

  commit: async (message) => {
    const root = useWorkspaceStore.getState().root;
    if (!root) return;
    await ipc.gitCommit(root, message);
    await get().refresh();
  },

  reset: () => set({ repo: false, branch: null, changes: [] }),
}));
