import { create } from "zustand";
import { ipc } from "../ipc";

export interface Term {
  id: number;
  name: string;
}

interface TerminalState {
  terms: Term[];
  activeId: number | null;
  create: (cwd?: string, shell?: string) => Promise<void>;
  setActive: (id: number) => void;
  kill: (id: number) => Promise<void>;
  removeLocal: (id: number) => void;
  rename: (id: number, name: string) => void;
}

export const useTerminalStore = create<TerminalState>((set, get) => ({
  terms: [],
  activeId: null,

  create: async (cwd, shell) => {
    try {
      const info = await ipc.createPty(cwd, shell);
      set((s) => ({
        terms: [...s.terms, { id: info.id, name: info.shell }],
        activeId: info.id,
      }));
    } catch (e) {
      console.error("pty create failed", e);
    }
  },

  setActive: (id) => set({ activeId: id }),

  kill: async (id) => {
    try {
      await ipc.killPty(id);
    } finally {
      get().removeLocal(id);
    }
  },

  removeLocal: (id) => {
    set((s) => {
      const terms = s.terms.filter((t) => t.id !== id);
      const activeId =
        s.activeId === id ? (terms.length ? terms[terms.length - 1].id : null) : s.activeId;
      return { terms, activeId };
    });
  },

  rename: (id, name) =>
    set((s) => ({
      terms: s.terms.map((t) => (t.id === id ? { ...t, name } : t)),
    })),
}));
