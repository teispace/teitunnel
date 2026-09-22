import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** UI-only state (panes, sizes). Server data never goes here (CONVENTIONS). */
interface UiState {
  sidebarCollapsed: boolean;
  inspectorOpen: boolean;
  toggleSidebar: () => void;
  toggleInspector: () => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      inspectorOpen: true,
      toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
      toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
    }),
    {
      name: "teitunnel.ui",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: ({ sidebarCollapsed, inspectorOpen }) => ({ sidebarCollapsed, inspectorOpen }),
    },
  ),
);
