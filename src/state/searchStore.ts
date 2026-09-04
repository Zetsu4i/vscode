import { create } from "zustand";
import { ipc, SearchHit } from "../ipc";
import { useWorkspaceStore } from "./workspaceStore";
import { useUiStore } from "./uiStore";

interface SearchState {
  query: string;
  replace: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
  results: SearchHit[];
  searching: boolean;
  replacing: boolean;
  filesScanned: number;
  truncated: boolean;

  setQuery: (q: string) => void;
  setReplace: (r: string) => void;
  toggleCase: () => void;
  toggleWord: () => void;
  toggleRegex: () => void;
  setResults: (r: SearchHit[]) => void;
  setProgress: (p: { filesScanned: number; hits: number }) => void;
  finish: (p: { hits: SearchHit[]; filesScanned: number; truncated: boolean }) => void;
  run: () => Promise<void>;
  replaceAll: () => Promise<void>;
}

export const useSearchStore = create<SearchState>((set, get) => ({
  query: "",
  replace: "",
  caseSensitive: false,
  wholeWord: false,
  regex: false,
  results: [],
  searching: false,
  replacing: false,
  filesScanned: 0,
  truncated: false,

  setQuery: (q) => set({ query: q }),
  setReplace: (r) => set({ replace: r }),
  toggleCase: () => set((s) => ({ caseSensitive: !s.caseSensitive })),
  toggleWord: () => set((s) => ({ wholeWord: !s.wholeWord })),
  toggleRegex: () => set((s) => ({ regex: !s.regex })),
  setResults: (r) => set({ results: r }),
  setProgress: (p) => set({ filesScanned: p.filesScanned }),
  finish: (p) =>
    set({
      results: p.hits,
      filesScanned: p.filesScanned,
      truncated: p.truncated,
      searching: false,
    }),

  run: async () => {
    const root = useWorkspaceStore.getState().root;
    const { query } = get();
    if (!root || !query) return;
    set({ searching: true, results: [], filesScanned: 0, truncated: false });
    try {
      await ipc.searchWorkspace(
        root,
        query,
        get().regex,
        get().caseSensitive,
        get().wholeWord
      );
    } catch (e) {
      console.error("search failed", e);
      set({ searching: false });
      return;
    }
    // If the backend errors before spawning, searching may hang; the done
    // listener always resolves it. Keep a safety timeout as well.
    setTimeout(() => {
      if (get().searching && get().filesScanned === 0 && get().results.length === 0) {
        set({ searching: false });
      }
    }, 20000);
    useUiStore.getState().setView("search");
  },

  replaceAll: async () => {
    const root = useWorkspaceStore.getState().root;
    const { query, replace, results } = get();
    if (!root || !query || results.length === 0) return;

    const files = [...new Set(results.map((h) => h.file))];
    const ui = useUiStore.getState();
    const replacementsLabel = `${results.length} occurrence${results.length === 1 ? "" : "s"} across ${files.length} file${files.length === 1 ? "" : "s"}`;

    ui.showConfirm({
      title: "Replace All",
      message: `Replace ${replacementsLabel}? This writes the changes to disk. Undirty open buffers are skipped.`,
      okLabel: "Replace",
      onOk: async () => {
        set({ replacing: true });
        try {
          const res = await ipc.replaceAll(
            root,
            query,
            replace,
            get().regex,
            get().caseSensitive,
            get().wholeWord
          );
          // Pull changed files back into any open (clean) buffers so the
          // editors and the LSP stay in sync with disk.
          const { useEditorStore } = await import("./editorStore");
          await useEditorStore.getState().reloadBuffers(res.filesChanged);
          // Refresh the result set against the now-updated workspace.
          await get().run();
        } catch (e) {
          console.error("replace failed", e);
        } finally {
          set({ replacing: false });
        }
      },
    });
  },
}));
