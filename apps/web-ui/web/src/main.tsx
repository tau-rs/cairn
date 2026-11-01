import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./app/App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "@fontsource-variable/inter";
import "./index.css";
import "./components/editor/livePreview.css";
import { installGlobalRejectionHandler } from "./app/globalErrorHandler";

installGlobalRejectionHandler();
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <HashRouter>
        <App />
      </HashRouter>
    </ErrorBoundary>
  </React.StrictMode>,
);
