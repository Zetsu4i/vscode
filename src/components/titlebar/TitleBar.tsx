import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useUiStore } from "../../state/uiStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useEditorStore, selectActiveKey } from "../../state/editorStore";
import { pickAndOpenFolder } from "../../commands";

interface MenuDef {
  label: string;
  items: { label: string; action?: () => void; separator?: boolean }[];
}

export default function TitleBar() {
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const [maximized, setMaximized] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  const root = useWorkspaceStore((s) => s.root);
  const rootName = useWorkspaceStore((s) => s.rootName);
  const activeKey = useEditorStore(selectActiveKey);
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

  const menus: MenuDef[] = [
    {
      label: "File",
      items: [
        { label: "Open Folder...", action: () => void pickAndOpenFolder() },
        { label: "", separator: true },
        {
          label: "Save",
          action: () => void useEditorStore.getState().save(),
        },
        {
          label: "Save All",
          action: () => void useEditorStore.getState().saveAll(),
        },
        { label: "", separator: true },
        {
          label: "Close Folder",
          action: () => useWorkspaceStore.getState().closeFolder(),
        },
      ],
    },
    {
      label: "View",
      items: [
        {
          label: "Command Palette...",
          action: () => useUiStore.getState().openPalette("commands"),
        },
        { label: "", separator: true },
        { label: "Explorer", action: () => useUiStore.getState().setView("explorer") },
        { label: "Search", action: () => useUiStore.getState().setView("search") },
        { label: "Source Control", action: () => useUiStore.getState().setView("git") },
        { label: "Extensions", action: () => useUiStore.getState().setView("extensions") },
        { label: "", separator: true },
        { label: "Terminal", action: () => useUiStore.getState().setPanelTab("terminal") },
        { label: "Problems", action: () => useUiStore.getState().setPanelTab("problems") },
      ],
    },
    {
      label: "Help",
      items: [
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
      ],
    },
  ];

  const title = rootName
    ? `${activeKey ? "" : ""}${rootName} - VSTauri`
    : "VSTauri";

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
                <div className="menu-dropdown">
                  {m.items.map((it, i) =>
                    it.separator ? (
                      <div key={i} className="menu-sep" />
                    ) : (
                      <button
                        key={i}
                        className="menu-item"
                        onClick={() => {
                          setMenuOpen(null);
                          it.action?.();
                        }}
                      >
                        {it.label}
                      </button>
                    )
                  )}
                </div>
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
