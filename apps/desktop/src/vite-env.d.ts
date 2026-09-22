/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" in E2E builds only (see `src-tauri/e2e/tauri.e2e.json`). */
  readonly VITE_E2E?: string;
}
