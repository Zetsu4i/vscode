import { useUiStore } from "../../state/uiStore";
import { useRef, useCallback, useEffect } from "react";
import ExplorerView from "./ExplorerView";
import SearchView from "./SearchView";
import GitView from "./GitView";
import ExtensionsView from "./ExtensionsView";

export default function Sidebar() {
  const view = useUiStore((s) => s.view);
  const width = useUiStore((s) => s.sidebarWidth);
  const setWidth = useUiStore((s) => s.setSidebarWidth);
  const dragging = useRef(false);

  const onMove = useCallback(
    (e: MouseEvent) => {
      if (dragging.current) setWidth(e.clientX - 48); // minus activity bar
    },
    [setWidth]
  );

  const onUp = useCallback(() => {
    dragging.current = false;
    document.body.classList.remove("resizing-col");
  }, []);

  useEffect(() => {
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [onMove, onUp]);

  return (
    <>
      <div className="sidebar" style={{ width }}>
        {view === "explorer" && <ExplorerView />}
        {view === "search" && <SearchView />}
        {view === "git" && <GitView />}
        {view === "extensions" && <ExtensionsView />}
      </div>
      <div
        className="sidebar-resizer"
        onMouseDown={() => {
          dragging.current = true;
          document.body.classList.add("resizing-col");
        }}
      />
    </>
  );
}
