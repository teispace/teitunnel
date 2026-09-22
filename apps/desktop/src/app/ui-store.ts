import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** UI-only state (panes, sizes). Server data never goes here (CONVENTIONS). */
interface UiState {
  sidebarCollapsed: boolean;
  inspectorOpen: boolean;
  paneSizes: Record<string, number>;
  paletteOpen: boolean;
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
      partialize: ({ sidebarCollapsed, inspectorOpen, paneSizes }) => ({
        sidebarCollapsed,
        inspectorOpen,
        paneSizes,
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
