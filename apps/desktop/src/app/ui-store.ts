import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** UI-only state (panes, sizes). Server data never goes here (CONVENTIONS). */
interface UiState {
  sidebarCollapsed: boolean;
  inspectorOpen: boolean;
  paneSizes: Record<string, number>;
  paletteOpen: boolean;
  /** The Cloudflare account shown in Domains/Routes. */
  activeAccountId: string | null;
  /** Doctor issues the user chose to ignore (stable issue ids). */
  ignoredIssues: string[];
  setIgnored: (id: string, ignored: boolean) => void;
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
      activeAccountId: null,
      ignoredIssues: [],
      setIgnored: (id, ignored) =>
        set((state) => ({
          ignoredIssues: ignored
            ? [...new Set([...state.ignoredIssues, id])]
            : state.ignoredIssues.filter((x) => x !== id),
        })),
      setActiveAccountId: (activeAccountId) => set({ activeAccountId }),
      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
      toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
      toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
      setPaneSize: (key, size) =>
        set((state) => ({ paneSizes: { ...state.paneSizes, [key]: size } })),
    }),
    {
      name: "teitunnel.ui",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: ({
        sidebarCollapsed,
        inspectorOpen,
        paneSizes,
        activeAccountId,
        ignoredIssues,
      }) => ({
        sidebarCollapsed,
        inspectorOpen,
        paneSizes,
        activeAccountId,
        ignoredIssues,
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
