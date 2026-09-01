import React from "react";
import { createRoot } from "react-dom/client";
import "@vscode/codicons/dist/codicon.css";
import "@xterm/xterm/css/xterm.css";
import "./styles/workbench.css";
import "./monaco";
import App from "./App";

createRoot(document.getElementById("root")!).render(<App />);
