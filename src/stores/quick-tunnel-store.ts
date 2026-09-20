import { create } from "zustand";
import { tauriApi, type QuickTunnelState } from "@/lib/tauri";

interface QuickTunnelStore {
  state: QuickTunnelState | null;
  localPort: number;
  isStarting: boolean;
  error: string | null;

  setPort: (port: number) => void;
  start: (port?: number) => Promise<void>;
  stop: () => Promise<void>;
  fetchState: () => Promise<void>;
  setPublicUrl: (url: string) => void;
  setStopped: () => void;
}

export const useQuickTunnelStore = create<QuickTunnelStore>((set, get) => ({
  state: null,
  localPort: 3000,
  isStarting: false,
  error: null,

  setPort: (port: number) => set({ localPort: port }),

  start: async (port?: number) => {
    const targetPort = port || get().localPort;
    try {
      set({ isStarting: true, error: null });
      const state = await tauriApi.startQuickTunnel(targetPort);
      set({ state, isStarting: false });
    } catch (err: unknown) {
      set({
        error: err instanceof Error ? err.message : String(err),
        isStarting: false,
      });
    }
  },

  stop: async () => {
    try {
      await tauriApi.stopQuickTunnel();
      set({
        state: null,
        isStarting: false,
      });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    }
  },

  fetchState: async () => {
    try {
      const state = await tauriApi.getQuickTunnelState();
      set({ state });
    } catch (err: unknown) {
      console.error("Failed to fetch quick tunnel state:", err);
    }
  },

  setPublicUrl: (url: string) => {
    set((s) => ({
      state: s.state
        ? { ...s.state, public_url: url, is_running: true }
        : {
            is_running: true,
            local_port: s.localPort,
            public_url: url,
            logs: [],
          },
      isStarting: false,
    }));
  },

  setStopped: () => {
    set((s) => ({
      state: s.state ? { ...s.state, is_running: false } : null,
      isStarting: false,
    }));
  },
}));
