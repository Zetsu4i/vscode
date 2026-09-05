import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useUiStore } from "../../state/uiStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useEditorStore } from "../../state/editorStore";
import { useTerminalStore } from "../../state/terminalStore";
import { runCommand, pickAndOpenFolder } from "../../commands";
import {
  editorUndo,
  editorRedo,
  editorCut,
  editorCopy,
  editorPaste,
  editorFind,
  editorReplace,
  editorFormat,
  editorSelectAll,
  editorGoToLineCol,
  editorAction,
} from "../../editorBridge";

interface MenuDef {
  label: string;
  items: MenuItemDef[];
}

interface MenuItemDef {
  label?: string;
  kb?: string;
  action?: () => void;
  separator?: boolean;
  children?: MenuItemDef[];
}

export default function TitleBar() {
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const [maximized, setMaximized] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  const root = useWorkspaceStore((s) => s.root);
  const rootName = useWorkspaceStore((s) => s.rootName);
  const activeKey = useEditorStore((s) => s.activeKey);
  const showConfirm = useUiStore((s) => s.showConfirm);

  useEffect(() => {
    const un = getCurrentWindow().onResized(async () => {
      setMaximized(await getCurrentWindow().isMaximized());
    });
    getCurrentWindow()
      .isMaximized()
      .then(setMaximized)
      .catch(() => {});
    return () => {
      void un;
    };
  }, []);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (e: MouseEvent) => {
      if (!barRef.current?.contains(e.target as Node)) setMenuOpen(null);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menuOpen]);

  const comingSoon = (feature: string) => () =>
    showConfirm({
      title: feature,
      message: `${feature} lands in a later phase of the rebuild. The workbench, filesystem, search, git, terminal and extension layers are already native.`,
      okLabel: "OK",
    });

  const menus: MenuDef[] = [
    {
      label: "File",
      items: [
        {
          label: "New File...",
          kb: "Ctrl+N",
          action: () => runCommand("workbench.action.files.newFile"),
        },
        { label: "Open Folder...", kb: "Ctrl+K Ctrl+O", action: () => void pickAndOpenFolder() },
        { label: "", separator: true },
        { label: "Save", kb: "Ctrl+S", action: () => void useEditorStore.getState().save() },
        { label: "Save All", kb: "Ctrl+K S", action: () => void useEditorStore.getState().saveAll() },
        { label: "", separator: true },
        {
          label: "Preferences",
          children: [
            { label: "Settings", kb: "Ctrl+,", action: () => runCommand("workbench.action.openSettings") },
            { label: "Color Theme...", kb: "Ctrl+K Ctrl+T", action: () => useUiStore.getState().openPalette("themes") },
            { label: "", separator: true },
            { label: "Open User Settings (JSON)", action: () => runCommand("workbench.action.openSettingsJson") },
            { label: "Open Workspace Settings (JSON)", action: () => runCommand("workbench.action.openWorkspaceSettingsJson") },
            { label: "", separator: true },
            { label: "Keyboard Shortcuts", action: comingSoon("Keyboard Shortcuts editor") },
          ],
        },
        { label: "", separator: true },
        { label: "Close Folder", kb: "Ctrl+K F", action: () => useWorkspaceStore.getState().closeFolder() },
        { label: "Exit", action: () => void getCurrentWindow().close() },
      ],
    },
    {
      label: "Edit",
      items: [
        { label: "Undo", kb: "Ctrl+Z", action: editorUndo },
        { label: "Redo", kb: "Ctrl+Y", action: editorRedo },
        { label: "", separator: true },
        { label: "Cut", kb: "Ctrl+X", action: editorCut },
        { label: "Copy", kb: "Ctrl+C", action: editorCopy },
        { label: "Paste", kb: "Ctrl+V", action: editorPaste },
        { label: "", separator: true },
        { label: "Find", kb: "Ctrl+F", action: editorFind },
        { label: "Replace", kb: "Ctrl+H", action: editorReplace },
        { label: "", separator: true },
        { label: "Format Document", kb: "Shift+Alt+F", action: editorFormat },
      ],
    },
    {
      label: "Selection",
      items: [
        { label: "Select All", kb: "Ctrl+A", action: editorSelectAll },
        { label: "", separator: true },
        { label: "Copy Line Up", kb: "Shift+Alt+↑", action: () => editorAction("editor.action.copyLinesUpAction") },
        { label: "Copy Line Down", kb: "Shift+Alt+↓", action: () => editorAction("editor.action.copyLinesDownAction") },
        { label: "Move Line Up", kb: "Alt+↑", action: () => editorAction("editor.action.moveLinesUpAction") },
        { label: "Move Line Down", kb: "Alt+↓", action: () => editorAction("editor.action.moveLinesDownAction") },
        { label: "", separator: true },
        { label: "Add Cursor Above", kb: "Ctrl+Alt+↑", action: () => editorAction("editor.action.insertCursorAbove") },
        { label: "Add Cursor Below", kb: "Ctrl+Alt+↓", action: () => editorAction("editor.action.insertCursorBelow") },
      ],
    },
    {
      label: "View",
      items: [
        {
          label: "Command Palette...",
          kb: "Ctrl+Shift+P",
          action: () => useUiStore.getState().openPalette("commands"),
        },
        { label: "", separator: true },
        { label: "Explorer", kb: "Ctrl+Shift+E", action: () => useUiStore.getState().setView("explorer") },
        { label: "Search", kb: "Ctrl+Shift+F", action: () => useUiStore.getState().setView("search") },
        { label: "Source Control", kb: "Ctrl+Shift+G", action: () => useUiStore.getState().setView("git") },
        { label: "Extensions", kb: "Ctrl+Shift+X", action: () => useUiStore.getState().setView("extensions") },
        { label: "", separator: true },
        { label: "Problems", kb: "Ctrl+Shift+M", action: () => useUiStore.getState().setPanelTab("problems") },
        { label: "Terminal", kb: "Ctrl+`", action: () => runCommand("workbench.action.terminal.toggleTerminal") },
        { label: "", separator: true },
        { label: "Toggle Primary Side Bar", kb: "Ctrl+B", action: () => useUiStore.getState().toggleSidebar() },
        { label: "Toggle Panel", kb: "Ctrl+J", action: () => useUiStore.getState().togglePanel() },
        { label: "", separator: true },
        { label: "Zoom In", kb: "Ctrl+=", action: () => runCommand("workbench.action.zoomIn") },
        { label: "Zoom Out", kb: "Ctrl+-", action: () => runCommand("workbench.action.zoomOut") },
        { label: "Reset Zoom", kb: "Ctrl+0", action: () => runCommand("workbench.action.zoomReset") },
      ],
    },
    {
      label: "Go",
      items: [
        {
          label: "Go to File...",
          kb: "Ctrl+P",
          action: () => useUiStore.getState().openPalette("files"),
        },
        { label: "Go to Line/Column...", kb: "Ctrl+G", action: editorGoToLineCol },
      ],
    },
    {
      label: "Run",
      items: [
        { label: "Start Debugging", kb: "F5", action: comingSoon("Debug Adapter Protocol support") },
        { label: "Run Without Debugging", kb: "Ctrl+F5", action: comingSoon("Debug Adapter Protocol support") },
        { label: "", separator: true },
        { label: "Run Task...", action: comingSoon("Task runner") },
      ],
    },
    {
      label: "Terminal",
      items: [
        { label: "New Terminal", kb: "Ctrl+Shift+`", action: () => runCommand("workbench.action.terminal.new") },
        { label: "", separator: true },
        {
          label: "Kill the Active Terminal",
          action: () => {
            const ts = useTerminalStore.getState();
            if (ts.activeId !== null) void ts.kill(ts.activeId);
          },
        },
      ],
    },
    {
      label: "Help",
      items: [
        {
          label: "Welcome",
          action: () => useEditorStore.getState().closeAll(),
        },
        {
          label: "Show All Commands",
          kb: "Ctrl+Shift+P",
          action: () => useUiStore.getState().openPalette("commands"),
        },
        { label: "", separator: true },
        {
          label: "About",
          action: () =>
            showConfirm({
              title: "VSTauri",
              message:
                "VSTauri 0.1.0 — a from-scratch VSCode-style workbench rebuilt on Tauri 2 + Rust. Lighter, faster, native. Unaffiliated with Microsoft.",
              okLabel: "OK",
            }),
        },
        { label: "Reload Window", kb: "Ctrl+R", action: () => window.location.reload() },
      ],
    },
  ];

  const title = rootName ? `${rootName} - VSTauri` : "VSTauri";

  const renderItems = (items: MenuItemDef[], nested = false) =>
    items.map((it, i) =>
      it.separator ? (
        <div key={i} className="menu-sep" />
      ) : it.children ? (
        <div key={i} className="menu-wrap menu-wrap-sub">
          <button className="menu-item menu-item-expand">
            <span className="menu-item-label">{it.label}</span>
            <span className="menu-item-kb">›</span>
          </button>
          <div className={`menu-dropdown submenu ${nested ? "submenu-nested" : ""}`}>
            {renderItems(it.children, true)}
          </div>
        </div>
      ) : (
        <button
          key={i}
          className="menu-item"
          onClick={() => {
            setMenuOpen(null);
            it.action?.();
          }}
        >
          <span className="menu-item-label">{it.label}</span>
          {it.kb && <span className="menu-item-kb">{it.kb}</span>}
        </button>
      )
    );

  return (
    <div className="titlebar" data-tauri-drag-region ref={barRef}>
      <div className="titlebar-left" data-tauri-drag-region>
        <div className="titlebar-logo">{"</>"}</div>
        <div className="titlebar-menubar">
          {menus.map((m) => (
            <div key={m.label} className="menu-wrap">
              <button
                className={`menu-button ${menuOpen === m.label ? "active" : ""}`}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  setMenuOpen(menuOpen === m.label ? null : m.label);
                }}
                onMouseEnter={() => menuOpen && setMenuOpen(m.label)}
              >
                {m.label}
              </button>
              {menuOpen === m.label && (
                <div className="menu-dropdown">{renderItems(m.items)}</div>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="titlebar-center" data-tauri-drag-region>
        <span className="titlebar-title">{title}</span>
      </div>

      <div className="titlebar-right">
        {!root && (
          <button
            className="titlebar-action"
            title="Open Folder"
            onClick={() => void pickAndOpenFolder()}
          >
            Open Folder
          </button>
        )}
        <button
          className="titlebar-btn"
          title="Minimize"
          onClick={() => void getCurrentWindow().minimize()}
        >
          <i className="codicon codicon-chrome-minimize" />
        </button>
        <button
          className="titlebar-btn"
          title={maximized ? "Restore" : "Maximize"}
          onClick={() => void getCurrentWindow().toggleMaximize()}
        >
          <i
            className={`codicon ${maximized ? "codicon-chrome-restore" : "codicon-chrome-maximize"}`}
          />
        </button>
        <button
          className="titlebar-btn titlebar-close"
          title="Close"
          onClick={() => void getCurrentWindow().close()}
        >
          <i className="codicon codicon-chrome-close" />
        </button>
      </div>
    </div>
  );
}
