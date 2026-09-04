import { ipc } from "../ipc";
import {
  useEditorStore,
  SessionSnapshot,
  LayoutNode,
} from "../state/editorStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";

/**
 * Session restore — the VSCode "reopen the workspace the way you left it"
 * behavior. Per-workspace state (open editors, active tab, split layout)
 * lives in <root>/.vstauri/session.json; window chrome (sidebar, panel)
 * is window-local and stored in localStorage.
 */

const SESSION_FILE = "session.json";
const LS_LAYOUT = "vstauri.layout.v2";

let restoring = false;
let saveTimer: number | undefined;
let layoutTimer: number | undefined;
let started = false;

// ---- serialize ---------------------------------------------------------------

function mapGroupIdsToIndices(node: LayoutNode, groups: { id: number }[]): LayoutNode {
  if (node.kind === "leaf") {
    const index = groups.findIndex((g) => g.id === node.groupId);
    return { kind: "leaf", groupId: Math.max(0, index) };
  }
  return {
    ...node,
    children: node.children.map((c) => mapGroupIdsToIndices(c, groups)),
  };
}

export function serializeSession(): SessionSnapshot {
  const s = useEditorStore.getState();
  return {
    groups: s.groups.map((g) => ({
      tabs: g.tabs.filter((t) => t.kind === "file").map((t) => t.path),
      active: g.activeKey?.startsWith("diff:") ? null : g.activeKey,
    })),
    activeGroupIndex: Math.max(
      0,
      s.groups.findIndex((g) => g.id === s.activeGroupId)
    ),
    layout: mapGroupIdsToIndices(s.layout, s.groups),
  };
}

// ---- save ----------------------------------------------------------------------

function scheduleSave(): void {
  if (restoring) return;
  const root = useWorkspaceStore.getState().root;
  if (!root) return;
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    const rootNow = useWorkspaceStore.getState().root;
    if (!rootNow) return;
    void ipc
      .configWrite("workspace", SESSION_FILE, serializeSession(), rootNow)
      .catch(() => {
        /* session persistence is best-effort */
      });
  }, 500);
}

// ---- restore ----------------------------------------------------------------------

export async function restoreSessionForWorkspace(): Promise<void> {
  const root = useWorkspaceStore.getState().root;
  const editor = useEditorStore.getState();
  if (!root) return;
  restoring = true;
  try {
    const doc = await ipc.configRead("workspace", SESSION_FILE, root);
    editor.resetForSession();
    if (
      doc &&
      typeof doc === "object" &&
      Array.isArray((doc as SessionSnapshot).groups) &&
      (doc as SessionSnapshot).groups.length > 0
    ) {
      await editor.restoreSession(doc as SessionSnapshot);
    }
  } catch {
    /* no session file — fresh workspace */
  } finally {
    // let the debounced saver skip one cycle, then resume tracking
    window.setTimeout(() => {
      restoring = false;
    }, 700);
  }
}

// ---- window layout (localStorage) -----------------------------------------------

interface WindowLayout {
  view: string;
  sidebarVisible: boolean;
  sidebarWidth: number;
  panelVisible: boolean;
  panelHeight: number;
  panelTab: string;
}

export function restoreWindowLayout(): void {
  try {
    const raw = localStorage.getItem(LS_LAYOUT);
    if (!raw) return;
    const l = JSON.parse(raw) as Partial<WindowLayout>;
    const ui = useUiStore.getState();
    if (typeof l.sidebarVisible === "boolean" && !l.sidebarVisible) ui.toggleSidebar();
    if (typeof l.sidebarWidth === "number" && l.sidebarWidth >= 170 && l.sidebarWidth <= 640) {
      ui.setSidebarWidth(l.sidebarWidth);
    }
    if (typeof l.panelVisible === "boolean" && l.panelVisible) {
      ui.setPanelTab(l.panelTab === "problems" ? "problems" : "terminal");
    }
    if (typeof l.panelHeight === "number" && l.panelHeight >= 120) {
      ui.setPanelHeight(l.panelHeight);
    }
    if (l.view === "search" || l.view === "git" || l.view === "extensions") {
      ui.setView(l.view);
    }
  } catch {
    /* corrupted layout — defaults */
  }
}

function scheduleLayoutSave(): void {
  window.clearTimeout(layoutTimer);
  layoutTimer = window.setTimeout(() => {
    const ui = useUiStore.getState();
    const l: WindowLayout = {
      view: ui.view,
      sidebarVisible: ui.sidebarVisible,
      sidebarWidth: ui.sidebarWidth,
      panelVisible: ui.panelVisible,
      panelHeight: ui.panelHeight,
      panelTab: ui.panelTab,
    };
    try {
      localStorage.setItem(LS_LAYOUT, JSON.stringify(l));
    } catch {
      /* storage unavailable */
    }
  }, 400);
}

// ---- startup ----------------------------------------------------------------------

/** Wire editor + ui subscriptions. Call once from App. */
export function startSessionTracking(): void {
  if (started) return;
  started = true;

  restoreWindowLayout();
  useUiStore.subscribe(scheduleLayoutSave);
  useEditorStore.subscribe((s, prev) => {
    if (s.groups !== prev.groups || s.activeGroupId !== prev.activeGroupId) {
      scheduleSave();
    }
  });
  // resave when a folder opens (fresh empty state) and after restore
  useWorkspaceStore.subscribe((s, prev) => {
    if (s.root !== prev.root) scheduleSave();
  });
}
