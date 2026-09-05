import React from "react";

interface State {
  error: Error | null;
}

/**
 * Top-level error boundary: if any part of the workbench throws, show a
 * recoverable message instead of a silently blank window.
 */
export class WorkbenchErrorBoundary extends React.Component<
  { children: React.ReactNode },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error): void {
    // eslint-disable-next-line no-console
    console.error("workbench crash:", error);
  }

  render(): React.ReactNode {
    if (this.state.error) {
      return (
        <div
          style={{
            height: "100vh",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 12,
            background: "#1f1f1f",
            color: "#cccccc",
            fontFamily: "sans-serif",
            padding: 24,
            textAlign: "center",
          }}
        >
          <div style={{ fontSize: 16, fontWeight: 600 }}>
            The workbench hit an unexpected error
          </div>
          <div style={{ color: "#9d9d9d", fontSize: 12, maxWidth: 640, wordBreak: "break-all" }}>
            {this.state.error.message}
          </div>
          <button
            onClick={() => window.location.reload()}
            style={{
              marginTop: 8,
              background: "#0078d4",
              color: "#ffffff",
              border: "none",
              borderRadius: 2,
              padding: "6px 16px",
              fontSize: 13,
              cursor: "pointer",
            }}
          >
            Reload Window
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
