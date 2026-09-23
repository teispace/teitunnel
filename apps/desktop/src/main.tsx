import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/app";
import { installErrorReporting } from "@/app/error-reporting";
import { repairNavigatorLanguage } from "@/app/locale";
import { isTauri } from "@/app/platform";
import { pickLanguage, setLanguage } from "@/lib/i18n";
import "@/styles/globals.css";

installErrorReporting();
// Before any view (and the libraries it loads) reads the language.
repairNavigatorLanguage();

async function mount() {
  await setLanguage(pickLanguage(navigator.languages));
  // In a plain browser during development (screenshots, design review), serve fixture
  // data instead of the Rust backend. Tree-shaken from release builds.
  if (import.meta.env.DEV && !isTauri()) {
    const { installMockIpc } = await import("@/dev/mock-ipc");
    installMockIpc();
  }
  // E2E builds only (`vite build --mode e2e`): the WebdriverIO bridge, tree-shaken otherwise.
  if (import.meta.env.MODE === "e2e") {
    await import("@wdio/tauri-plugin");
  }
  const root = document.getElementById("root");
  if (root) {
    createRoot(root).render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
  }
}

void mount();
