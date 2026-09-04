import { create } from "zustand";
import { ipc } from "../ipc";
import { baseName } from "../util/paths";

export type TabKind = "file" | "diff" | "settings" | "keybindings";

export interface OpenTab {
  /** path for files, "diff:<path>" for diffs, "settings"/"keybindings" for workbench tabs */
  key: string;
  kind: TabKind;
  path: string;
}

/** One editor group: an independent tab strip + one visible editor. */
export interface EditorGroup {
  id: number;
  tabs: OpenTab[];
  activeKey: string | null;
}

/**
 * Workbench grid. A leaf hosts one editor group; a split lays its children
 * out along an axis with flexible size weights (percentages).
 */
export type LayoutNode =
  | { kind: "leaf"; groupId: number }
  | {
      kind: "split";
      id: number;
      dir: "row" | "column"; // row = side by side, column = stacked
      children: LayoutNode[];
      sizes: number[];
    };

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

/** Serializable workbench state for session restore (group ids are indices). */
export interface SessionSnapshot {
  groups: { tabs: string[]; active: string | null }[];
  activeGroupIndex: number;
  layout: LayoutNode;
}

interface EditorState {
  groups: EditorGroup[];
  activeGroupId: number;
  layout: LayoutNode;
  nextGroupId: number;
  nextSplitId: number;
  buffers: Record<string, FileBuf>;
  problems: Problem[];
  /** head (committed) content cache for diff tabs */
  diffBase: Record<string, string>;

  openFile: (path: string) => Promise<void>;
  openDiff: (path: string) => Promise<void>;
  /** Open a workbench UI tab (Settings, Keyboard Shortcuts) in the active group. */
  openSpecial: (kind: "settings" | "keybindings") => void;
  closeTab: (key: string) => void;
  closeAll: () => void;
  setActive: (key: string) => void;
  setText: (path: string, text: string) => void;
  markSaved: (path: string) => void;
  save: (path?: string) => Promise<boolean>;
  saveAll: () => Promise<void>;
  /** Re-read the given files from disk into open buffers (skips dirty ones). */
  reloadBuffers: (paths: string[]) => Promise<void>;
  handleRename: (oldPath: string, newPath: string) => void;
  handleDelete: (path: string) => void;
  setProblems: (p: Problem[]) => void;

  focusGroup: (groupId: number) => void;
  splitGroup: (dir: "right" | "down") => void;
  closeGroup: (groupId: number) => void;
  /** Restore a persisted session (replaces all groups/layout/buffers). */
  restoreSession: (sess: SessionSnapshot) => Promise<void>;
  /** Reset to a single empty group (used on folder close/switch). */
  resetForSession: () => void;
  reorderTab: (groupId: number, from: number, to: number) => void;
  moveTabToGroup: (key: string, fromGroupId: number, toGroupId: number, index?: number) => void;
  resizeSplit: (splitId: number, index: number, deltaPx: number, containerPx: number) => void;
}

// ---- layout tree helpers (pure) --------------------------------------------

function replaceLeaf(node: LayoutNode, groupId: number, fn: (leaf: LayoutNode) => LayoutNode): LayoutNode {
  if (node.kind === "leaf") {
    return node.groupId === groupId ? fn(node) : node;
  }
  return {
    ...node,
    children: node.children.map((c) => replaceLeaf(c, groupId, fn)),
  };
}

/** Remove a group's leaf; collapse single-child splits. Returns null if the whole subtree is gone. */
function removeLeaf(node: LayoutNode, groupId: number): LayoutNode | null {
  if (node.kind === "leaf") return node.groupId === groupId ? null : node;
  const children: LayoutNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((c, i) => {
    const kept = removeLeaf(c, groupId);
    if (kept) {
      children.push(kept);
      sizes.push(node.sizes[i]);
    }
  });
  if (children.length === 0) return null;
  if (children.length === 1) return children[0];
  return { ...node, children, sizes: normalize(sizes) };
}

function mapSplits(node: LayoutNode, fn: (s: Extract<LayoutNode, { kind: "split" }>) => LayoutNode): LayoutNode {
  if (node.kind === "leaf") return node;
  const mapped = fn(node);
  if (mapped.kind !== "split") return mapped;
  return {
    ...mapped,
    children: mapped.children.map((c) => mapSplits(c, fn)),
  };
}

function normalize(sizes: number[]): number[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  if (total <= 0) return sizes.map(() => 100 / Math.max(1, sizes.length));
  return sizes;
}

function collectGroupIds(node: LayoutNode): number[] {
  if (node.kind === "leaf") return [node.groupId];
  return node.children.flatMap(collectGroupIds);
}

// ---- selectors --------------------------------------------------------------

export function selectActiveGroup(s: EditorState): EditorGroup | undefined {
  return s.groups.find((g) => g.id === s.activeGroupId);
}

export function selectActiveKey(s: EditorState): string | null {
  return selectActiveGroup(s)?.activeKey ?? null;
}

export function selectActiveTab(s: EditorState): OpenTab | null {
  const key = selectActiveKey(s);
  if (!key) return null;
  return selectActiveGroup(s)?.tabs.find((t) => t.key === key) ?? null;
}

function tabKey(path: string): string {
  return path;
}

export const useEditorStore = create<EditorState>((set, get) => ({
  groups: [{ id: 0, tabs: [], activeKey: null }],
  activeGroupId: 0,
  layout: { kind: "leaf", groupId: 0 },
  nextGroupId: 1,
  nextSplitId: 1,
  buffers: {},
  problems: [],
  diffBase: {},

  openFile: async (path) => {
    const key = tabKey(path);
    const s = get();
    // Already open somewhere? Focus that group + tab.
    const owner = s.groups.find((g) => g.tabs.some((t) => t.key === key));
    if (owner) {
      set((st) => ({
        activeGroupId: owner.id,
        groups: st.groups.map((g) => (g.id === owner.id ? { ...g, activeKey: key } : g)),
      }));
      return;
    }
    try {
      const fc = await ipc.readFile(path);
      set((st) => ({
        buffers: {
          ...st.buffers,
          [path]: {
            text: fc.content,
            savedText: fc.content,
            dirty: false,
            binary: fc.isBinary,
            truncated: fc.truncated,
            version: 1,
          },
        },
        groups: st.groups.map((g) =>
          g.id === st.activeGroupId
            ? { ...g, tabs: [...g.tabs, { key, kind: "file" as TabKind, path }], activeKey: key }
            : g
        ),
      }));
    } catch (e) {
      console.error("open failed", e);
    }
  },

  openDiff: async (path) => {
    const key = `diff:${path}`;
    const s = get();
    const owner = s.groups.find((g) => g.tabs.some((t) => t.key === key));
    if (owner) {
      set((st) => ({
        activeGroupId: owner.id,
        groups: st.groups.map((g) => (g.id === owner.id ? { ...g, activeKey: key } : g)),
      }));
      return;
    }
    const { useWorkspaceStore } = await import("./workspaceStore");
    const root = useWorkspaceStore.getState().root;
    if (!root) return;
    try {
      const base = await ipc.gitShowHead(root, path);
      set((st) => ({
        diffBase: { ...st.diffBase, [path]: base },
        groups: st.groups.map((g) =>
          g.id === st.activeGroupId
            ? { ...g, tabs: [...g.tabs, { key, kind: "diff" as TabKind, path }], activeKey: key }
            : g
        ),
      }));
    } catch (e) {
      console.error("diff open failed", e);
    }
  },

  openSpecial: (kind) => {
    const key = kind;
    const s = get();
    const owner = s.groups.find((g) => g.tabs.some((t) => t.key === key));
    if (owner) {
      set((st) => ({
        activeGroupId: owner.id,
        groups: st.groups.map((g) => (g.id === owner.id ? { ...g, activeKey: key } : g)),
      }));
      return;
    }
    set((st) => ({
      groups: st.groups.map((g) =>
        g.id === st.activeGroupId
          ? { ...g, tabs: [...g.tabs, { key, kind: kind as TabKind, path: kind }], activeKey: key }
          : g
      ),
    }));
  },

  closeTab: (key) => {
    set((s) => {
      const owner = s.groups.find((g) => g.tabs.some((t) => t.key === key));
      if (!owner) return s;
      const idx = owner.tabs.findIndex((t) => t.key === key);
      const tabs = owner.tabs.filter((t) => t.key !== key);
      let groups: EditorGroup[];
      let layout = s.layout;
      let activeGroupId = s.activeGroupId;

      if (tabs.length === 0 && s.groups.length > 1) {
        // Group becomes empty → remove the group and collapse the grid.
        groups = s.groups.filter((g) => g.id !== owner.id);
        const removed = removeLeaf(s.layout, owner.id);
        if (removed) layout = removed;
        if (activeGroupId === owner.id) {
          activeGroupId = groups[groups.length - 1].id;
        }
      } else {
        const activeKey =
          owner.activeKey === key
            ? tabs[Math.min(idx, tabs.length - 1)]?.key ?? null
            : owner.activeKey;
        groups = s.groups.map((g) => (g.id === owner.id ? { ...g, tabs, activeKey } : g));
      }
      return { groups, layout, activeGroupId };
    });
  },

  closeAll: () =>
    set((s) => ({
      groups: s.groups.map((g) => (g.id === 0 ? g : { ...g, tabs: [], activeKey: null })),
    })),

  setActive: (key) =>
    set((s) => {
      const owner = s.groups.find((g) => g.tabs.some((t) => t.key === key));
      if (!owner) return s;
      return {
        activeGroupId: owner.id,
        groups: s.groups.map((g) => (g.id === owner.id ? { ...g, activeKey: key } : g)),
      };
    }),

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
    const target = path ?? selectActiveKey(s);
    if (!target) return false;
    const buf = s.buffers[target];
    if (!buf) return false;
    try {
      await ipc.writeFile(target, buf.text);
      get().markSaved(target);
      // Notify the language server the document was saved.
      const { languageForPath } = await import("../util/paths");
      const lang = languageForPath(target);
      const { useWorkspaceStore } = await import("./workspaceStore");
      if (lang && useWorkspaceStore.getState().root) {
        void ipc.lspDidSave(lang, target).catch(() => {});
      }
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

  reloadBuffers: async (paths) => {
    const updates: Record<string, FileBuf> = {};
    for (const path of paths) {
      const buf = get().buffers[path];
      if (!buf || buf.dirty) continue; // never clobber in-progress edits
      try {
        const fc = await ipc.readFile(path);
        updates[path] = {
          ...buf,
          text: fc.content,
          savedText: fc.content,
          dirty: false,
          binary: fc.isBinary,
          truncated: fc.truncated,
          version: buf.version + 1,
        };
      } catch {
        /* file may have been deleted; the watcher will handle that */
      }
    }
    if (Object.keys(updates).length > 0) {
      set((s) => ({ buffers: { ...s.buffers, ...updates } }));
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
      const remap = (t: OpenTab): OpenTab => {
        if (t.key === oldPath) return { ...t, key: newPath, path: newPath };
        if (t.key === `diff:${oldPath}`) return { ...t, key: `diff:${newPath}`, path: newPath };
        return t;
      };
      const groups = s.groups.map((g) => {
        const tabs = g.tabs.map(remap);
        const activeKey =
          g.activeKey === oldPath
            ? newPath
            : g.activeKey === `diff:${oldPath}`
              ? `diff:${newPath}`
              : g.activeKey;
        return { ...g, tabs, activeKey };
      });
      return { buffers, diffBase, groups };
    });
  },

  handleDelete: (path) => {
    set((s) => {
      const groups = s.groups.map((g) => {
        const tabs = g.tabs.filter(
          (t) => t.path !== path && !t.path.startsWith(path + "/") && !t.path.startsWith(path + "\\")
        );
        const activeKey = tabs.some((t) => t.key === g.activeKey) ? g.activeKey : tabs[tabs.length - 1]?.key ?? null;
        return { ...g, tabs, activeKey };
      });
      return { groups };
    });
  },

  setProblems: (p) => set({ problems: p }),

  focusGroup: (groupId) =>
    set((s) => (s.activeGroupId === groupId ? s : { activeGroupId: groupId })),

  splitGroup: (dir) => {
    const s = get();
    const source = selectActiveGroup(s);
    if (!source) return;
    const newId = s.nextGroupId;
    const splitId = s.nextSplitId;
    // VSCode behavior: the new group opens with a copy of the active editor.
    const carried = source.activeKey
      ? source.tabs.filter((t) => t.key === source.activeKey)
      : [];
    const newGroup: EditorGroup = {
      id: newId,
      tabs: carried.map((t) => ({ ...t })),
      activeKey: carried[0]?.key ?? null,
    };
    const newLeaf: LayoutNode = { kind: "leaf", groupId: newId };
    const layout = replaceLeaf(s.layout, source.id, (leaf) => ({
      kind: "split",
      id: splitId,
      dir: dir === "right" ? "row" : "column",
      children: [leaf, newLeaf],
      sizes: [50, 50],
    }));
    set({
      groups: [...s.groups, newGroup],
      layout,
      nextGroupId: newId + 1,
      nextSplitId: splitId + 1,
      activeGroupId: newId,
    });
  },

  closeGroup: (groupId) => {
    set((s) => {
      if (s.groups.length <= 1) {
        // Last group: just clear its tabs.
        return {
          groups: s.groups.map((g) => (g.id === groupId ? { ...g, tabs: [], activeKey: null } : g)),
        };
      }
      const groups = s.groups.filter((g) => g.id !== groupId);
      const layout = removeLeaf(s.layout, groupId) ?? { kind: "leaf", groupId: groups[0].id };
      const remaining = collectGroupIds(layout);
      const activeGroupId = remaining.includes(s.activeGroupId)
        ? s.activeGroupId
        : remaining[remaining.length - 1];
      return { groups, layout, activeGroupId };
    });
  },

  reorderTab: (groupId, from, to) => {
    set((s) => ({
      groups: s.groups.map((g) => {
        if (g.id !== groupId || from === to) return g;
        const tabs = [...g.tabs];
        const [moved] = tabs.splice(from, 1);
        tabs.splice(to, 0, moved);
        return { ...g, tabs };
      }),
    }));
  },

  moveTabToGroup: (key, fromGroupId, toGroupId, index) => {
    set((s) => {
      if (fromGroupId === toGroupId) return s;
      const from = s.groups.find((g) => g.id === fromGroupId);
      const to = s.groups.find((g) => g.id === toGroupId);
      const tab = from?.tabs.find((t) => t.key === key);
      if (!from || !to || !tab) return s;
      let activeGroupId = s.activeGroupId;
      const groups = s.groups.map((g) => {
        if (g.id === fromGroupId) {
          const tabs = g.tabs.filter((t) => t.key !== key);
          const activeKey = g.activeKey === key ? tabs[tabs.length - 1]?.key ?? null : g.activeKey;
          return { ...g, tabs, activeKey };
        }
        if (g.id === toGroupId) {
          const tabs = [...g.tabs];
          tabs.splice(index ?? tabs.length, 0, tab);
          return { ...g, tabs, activeKey: tab.key };
        }
        return g;
      });
      if (toGroupId === s.activeGroupId) activeGroupId = toGroupId;
      return { groups, activeGroupId };
    });
  },

  resizeSplit: (splitId, index, deltaPx, containerPx) => {
    if (containerPx <= 0) return;
    set((s) => {
      const layout = mapSplits(s.layout, (node) => {
        if (node.id !== splitId) return node;
        const sizes = [...node.sizes];
        const total = sizes.reduce((a, b) => a + b, 0) || 100;
        const deltaPct = (deltaPx / containerPx) * total;
        const a = sizes[index];
        const b = sizes[index + 1];
        if (a === undefined || b === undefined) return node;
        const minPct = 5;
        const d = Math.max(-(b - minPct), Math.min(a - minPct, deltaPct));
        sizes[index] = a + d;
        sizes[index + 1] = b - d;
        return { ...node, sizes };
      });
      return { layout };
    });
  },

  resetForSession: () =>
    set(() => ({
      groups: [{ id: 0, tabs: [], activeKey: null }],
      activeGroupId: 0,
      layout: { kind: "leaf", groupId: 0 },
      nextGroupId: 1,
      nextSplitId: 1,
      buffers: {},
      problems: [],
      diffBase: {},
    })),

  restoreSession: async (sess) => {
    if (!Array.isArray(sess.groups) || sess.groups.length === 0) return;
    // Load every referenced file once, in parallel; drop missing ones.
    const paths = [...new Set(sess.groups.flatMap((g) => g.tabs))].filter(
      (p) => typeof p === "string"
    );
    const buffers: Record<string, FileBuf> = {};
    await Promise.all(
      paths.map(async (p) => {
        try {
          const fc = await ipc.readFile(p);
          buffers[p] = {
            text: fc.content,
            savedText: fc.content,
            dirty: false,
            binary: fc.isBinary,
            truncated: fc.truncated,
            version: 1,
          };
        } catch {
          /* file vanished since last session */
        }
      })
    );

    let maxSplitId = 0;
    const scan = (n: LayoutNode): void => {
      if (n.kind === "split") {
        maxSplitId = Math.max(maxSplitId, n.id);
        n.children.forEach(scan);
      }
    };
    scan(sess.layout);

    const groups: EditorGroup[] = sess.groups.map((g, i) => {
      const tabs = g.tabs.filter((p) => buffers[p]);
      const activeKey = g.active && buffers[g.active] ? g.active : tabs[tabs.length - 1] ?? null;
      return {
        id: i,
        tabs: tabs.map((p) => ({ key: p, kind: "file" as TabKind, path: p })),
        activeKey,
      };
    });
    const activeGroupIndex = Math.min(
      Math.max(0, sess.activeGroupIndex ?? 0),
      groups.length - 1
    );
    set({
      groups,
      layout: sess.layout,
      activeGroupId: groups[activeGroupIndex].id,
      nextGroupId: groups.length,
      nextSplitId: maxSplitId + 1,
      buffers,
    });
  },
}));

export function tabLabel(tab: OpenTab): string {
  if (tab.kind === "settings") return "Settings";
  if (tab.kind === "keybindings") return "Keyboard Shortcuts";
  const name = baseName(tab.path);
  return tab.kind === "diff" ? `${name} (Working Tree)` : name;
}
