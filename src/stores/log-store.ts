import { create } from "zustand";
import { type TunnelLogEvent } from "@/lib/tauri";

interface LogStore {
  logs: TunnelLogEvent[];
  filterLevel: string; // "ALL" | "INFO" | "WARN" | "ERR"
  searchQuery: string;
  selectedTunnelFilter: string; // "ALL" or specific tunnel_id
  isPaused: boolean;

  addLog: (log: TunnelLogEvent) => void;
  clearLogs: () => void;
  setFilterLevel: (level: string) => void;
  setSearchQuery: (query: string) => void;
  setSelectedTunnelFilter: (tunnelId: string) => void;
  togglePause: () => void;
}

export const useLogStore = create<LogStore>((set) => ({
  logs: [],
  filterLevel: "ALL",
  searchQuery: "",
  selectedTunnelFilter: "ALL",
  isPaused: false,

  addLog: (log) =>
    set((state) => {
      if (state.isPaused) return state;
      const next = [...state.logs, log];
      if (next.length > 2000) {
        next.splice(0, next.length - 2000);
      }
      return { logs: next };
    }),

  clearLogs: () => set({ logs: [] }),
  setFilterLevel: (filterLevel) => set({ filterLevel }),
  setSearchQuery: (searchQuery) => set({ searchQuery }),
  setSelectedTunnelFilter: (selectedTunnelFilter) => set({ selectedTunnelFilter }),
  togglePause: () => set((s) => ({ isPaused: !s.isPaused })),
}));
