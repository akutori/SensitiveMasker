import React from "react";
import ReactDOM from "react-dom/client";
import { RouterProvider, createRouter } from "@tanstack/react-router";
import { AppStateProvider } from "@/lib/app-state";
import { routeTree } from "./routeTree.gen";
import "./index.css";

// e2e-testing用ビルド(VITE_E2E_TESTING=true)でのみ読み込む。この定数分岐は
// ビルド時にリテラル化されるため、通常ビルドではimport自体がtree-shakeで
// 消え、配布物にwdioテストプラグインが含まれない。
if (import.meta.env.VITE_E2E_TESTING === "true") {
  import("@wdio/tauri-plugin");
}

const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppStateProvider>
      <RouterProvider router={router} />
    </AppStateProvider>
  </React.StrictMode>
);
