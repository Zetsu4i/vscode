import React from "react";
import { createRoot } from "react-dom/client";
import "./dev/browserShim";
import "@vscode/codicons/dist/codicon.css";
import "@xterm/xterm/css/xterm.css";
import "./styles/workbench.css";
import "./monaco";
import App from "./App";
import { WorkbenchErrorBoundary } from "./dev/ErrorBoundary";

createRoot(document.getElementById("root")!).render(
  <WorkbenchErrorBoundary>
    <App />
  </WorkbenchErrorBoundary>
);
