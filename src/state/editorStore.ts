import { create } from "zustand";
import { ipc } from "../ipc";
import { baseName } from "../util/paths";

export type TabKind = "file" | "diff";

export interface OpenTab {
  key: string; // path for files, "diff:<path>" for diffs
  kind: TabKind;
  path: string;
}

export interface FileBuf {
  text: string;
  savedText: string;
  dirty: boolean;
  binary: boolean;
  truncated: boolean;
  version: number;
}

export interface Problem {
  path: string;
  line: number;
  col: number;
  endLine: number;
  endCol: number;
  message: string;
  severity: number; // 1=Error 2=Warning 3=Info 4=Hint
  source?: string;
}

interface EditorState {
  tabs: OpenTab[];
  activeKey: string | null;
  buffers: Record<string, FileBuf>;
  problems: Problem[];
  /** head (committed) content cache for diff tabs */
  diffBase: Record<string, string>;

  openFile: (path: string) => Promise<void>;
  openDiff: (path: string) => Promise<void>;
  closeTab: (key: string) => void;
  closeAll: () => void;
  setActive: (key: string) => void;
  setText: (path: string, text: string) => void;
  markSaved: (path: string) => void;
  save: (path?: string) => Promise<boolean>;
  saveAll: () => Promise<void>;
  handleRename: (oldPath: string, newPath: string) => void;
  handleDelete: (path: string) => void;
  setProblems: (p: Problem[]) => void;
}

function tabKey(path: string): string {
  return path;
}

export const useEditorStore = create<EditorState>((set, get) => ({
  tabs: [],
  activeKey: null,
  buffers: {},
  problems: [],
  diffBase: {},

  openFile: async (path) => {
    const key = tabKey(path);
    const existing = get().tabs.find((t) => t.key === key);
    if (existing) {
      set({ activeKey: key });
      return;
    }
    try {
      const fc = await ipc.readFile(path);
      set((s) => ({
        buffers: {
          ...s.buffers,
          [path]: {
            text: fc.content,
            savedText: fc.content,
            dirty: false,
            binary: fc.isBinary,
            truncated: fc.truncated,
            version: 1,
          },
        },
        tabs: [...s.tabs, { key, kind: "file", path }],
        activeKey: key,
      }));
    } catch (e) {
      console.error("open failed", e);
    }
  },

  openDiff: async (path) => {
    const key = `diff:${path}`;
    const existing = get().tabs.find((t) => t.key === key);
    if (existing) {
      set({ activeKey: key });
      return;
    }
    const { useWorkspaceStore } = await import("./workspaceStore");
    const root = useWorkspaceStore.getState().root;
    if (!root) return;
    try {
      const base = await ipc.gitShowHead(root, path);
      set((s) => ({
        diffBase: { ...s.diffBase, [path]: base },
        tabs: [...s.tabs, { key, kind: "diff", path }],
        activeKey: key,
      }));
    } catch (e) {
      console.error("diff open failed", e);
    }
  },

  closeTab: (key) => {
    set((s) => {
      const tabs = s.tabs.filter((t) => t.key !== key);
      const activeKey =
        s.activeKey === key
          ? (tabs.length > 0
              ? tabs[Math.max(0, s.tabs.findIndex((t) => t.key === key) - 1)].key
              : null)
          : s.activeKey;
      return { tabs, activeKey };
    });
  },

  closeAll: () => set({ tabs: [], activeKey: null }),

  setActive: (key) => set({ activeKey: key }),

  setText: (path, text) => {
    set((s) => {
      const b = s.buffers[path];
      if (!b || b.text === text) return s;
      return {
        buffers: {
          ...s.buffers,
          [path]: { ...b, text, dirty: text !== b.savedText, version: b.version + 1 },
        },
      };
    });
  },

  markSaved: (path) => {
    set((s) => {
      const b = s.buffers[path];
      if (!b) return s;
      return {
        buffers: {
          ...s.buffers,
          [path]: { ...b, savedText: b.text, dirty: false },
        },
      };
    });
  },

  save: async (path) => {
    const s = get();
    const target = path ?? (s.activeKey?.startsWith("diff:") ? null : s.activeKey);
    if (!target) return false;
    const buf = s.buffers[target];
    if (!buf) return false;
    try {
      await ipc.writeFile(target, buf.text);
      get().markSaved(target);
      return true;
    } catch (e) {
      console.error("save failed", e);
      return false;
    }
  },

  saveAll: async () => {
    const dirty = Object.entries(get().buffers).filter(([, b]) => b.dirty);
    for (const [path] of dirty) {
      await get().save(path);
    }
  },

  handleRename: (oldPath, newPath) => {
    set((s) => {
      const buffers = { ...s.buffers };
      const buf = buffers[oldPath];
      if (buf) {
        delete buffers[oldPath];
        buffers[newPath] = buf;
      }
      const diffBase = { ...s.diffBase };
      if (diffBase[oldPath] !== undefined) {
        diffBase[newPath] = diffBase[oldPath];
        delete diffBase[oldPath];
      }
      const tabs = s.tabs.map((t) => {
        if (t.key === oldPath) return { ...t, key: newPath, path: newPath };
        if (t.key === `diff:${oldPath}`)
          return { ...t, key: `diff:${newPath}`, path: newPath };
        return t;
      });
      const activeKey =
        s.activeKey === oldPath
          ? newPath
          : s.activeKey === `diff:${oldPath}`
            ? `diff:${newPath}`
            : s.activeKey;
      return { buffers, diffBase, tabs, activeKey };
    });
  },

  handleDelete: (path) => {
    set((s) => ({
      tabs: s.tabs.filter(
        (t) => t.path !== path && !(t.path.startsWith(path + "/") || t.path.startsWith(path + "\\"))
      ),
      activeKey: s.tabs.find((t) => t.key === s.activeKey && t.path !== path)
        ? s.activeKey
        : null,
    }));
  },

  setProblems: (p) => set({ problems: p }),
}));

export function tabLabel(tab: OpenTab): string {
  const name = baseName(tab.path);
  return tab.kind === "diff" ? `${name} (Working Tree)` : name;
}
