import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/app";
import { installErrorReporting } from "@/app/error-reporting";
import { isTauri } from "@/app/platform";
import "@/styles/globals.css";

installErrorReporting();

async function mount() {
  // In a plain browser during development (screenshots, design review), serve fixture
  // data instead of the Rust backend. Tree-shaken from release builds.
  if (import.meta.env.DEV && !isTauri()) {
    const { installMockIpc } = await import("@/dev/mock-ipc");
    installMockIpc();
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
