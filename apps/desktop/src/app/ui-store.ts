import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** UI-only state (panes, sizes). Server data never goes here (CONVENTIONS). */
interface UiState {
  sidebarCollapsed: boolean;
  inspectorOpen: boolean;
  paneSizes: Record<string, number>;
  paletteOpen: boolean;
  /** The "quit while routes run" question is showing. */
  quitOpen: boolean;
  setQuitOpen: (open: boolean) => void;
  /** The Export Diagnostics dialog is showing (opened from the Doctor or Help menu). */
  diagnosticsOpen: boolean;
  setDiagnosticsOpen: (open: boolean) => void;
  /** The Cloudflare account shown in Domains/Routes. */
  activeAccountId: string | null;
  /**
   * Doctor issues ignored before ignores moved to settings. The Doctor moves
   * them to settings once and clears this; remove after v0.5.
   */
  legacyIgnoredIssues: string[];
  clearLegacyIgnoredIssues: () => void;
  setActiveAccountId: (id: string | null) => void;
  setPaletteOpen: (open: boolean) => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
  setPaneSize: (key: string, size: number) => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      inspectorOpen: true,
      paneSizes: {},
      paletteOpen: false,
      quitOpen: false,
      setQuitOpen: (quitOpen) => set({ quitOpen }),
      diagnosticsOpen: false,
      setDiagnosticsOpen: (diagnosticsOpen) => set({ diagnosticsOpen }),
      activeAccountId: null,
      legacyIgnoredIssues: [],
      clearLegacyIgnoredIssues: () => set({ legacyIgnoredIssues: [] }),
      setActiveAccountId: (activeAccountId) => set({ activeAccountId }),
      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
      toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
      toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
      setPaneSize: (key, size) =>
        set((state) => ({ paneSizes: { ...state.paneSizes, [key]: size } })),
    }),
    {
      name: "teitunnel.ui",
      version: 2,
      // v1 kept Doctor ignores here as `ignoredIssues`; hand them to the Doctor to move.
      migrate: (persisted, version) => {
        const state = (persisted ?? {}) as Record<string, unknown>;
        if (version < 2) {
          const { ignoredIssues, ...rest } = state;
          return {
            ...rest,
            legacyIgnoredIssues: Array.isArray(ignoredIssues) ? ignoredIssues : [],
          };
        }
        return state;
      },
      storage: createJSONStorage(() => localStorage),
      partialize: ({
        sidebarCollapsed,
        inspectorOpen,
        paneSizes,
        activeAccountId,
        legacyIgnoredIssues,
      }) => ({
        sidebarCollapsed,
        inspectorOpen,
        paneSizes,
        activeAccountId,
        legacyIgnoredIssues,
      }),
    },
  ),
);

/** A persisted pane size, falling back to `fallback` until the user resizes it. */
export function usePaneSize(key: string, fallback: number): [number, (size: number) => void] {
  const size = useUiStore((state) => state.paneSizes[key] ?? fallback);
  const setPaneSize = useUiStore((state) => state.setPaneSize);
  return [size, (next) => setPaneSize(key, next)];
}
