import { useEffect, useRef } from "react";
import { useUiStore } from "../../state/uiStore";

export default function ContextMenu() {
  const menu = useUiStore((s) => s.contextMenu);
  const close = useUiStore((s) => s.closeContextMenu);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu, close]);

  if (!menu) return null;

  // keep the menu inside the viewport
  const x = Math.min(menu.x, window.innerWidth - 240);
  const y = Math.min(menu.y, window.innerHeight - 40 - menu.items.length * 26);

  return (
    <div className="context-menu" ref={ref} style={{ left: x, top: y }}>
      {menu.items.map((item, i) =>
        item.separator ? (
          <div key={i} className="menu-sep" />
        ) : (
          <button
            key={i}
            className={`menu-item ${item.danger ? "danger" : ""}`}
            onClick={() => {
              close();
              item.action?.();
            }}
          >
            {item.icon ? (
              <i className={`codicon ${item.icon}`} style={{ width: 16 }} />
            ) : null}
            <span className="menu-item-label">{item.label}</span>
            {item.kb && <span className="menu-item-kb">{item.kb}</span>}
          </button>
        )
      )}
    </div>
  );
}
