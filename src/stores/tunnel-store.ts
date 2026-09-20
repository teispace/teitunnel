import { create } from "zustand";
import { tauriApi, type CloudflareTunnel, type TunnelProcessState } from "@/lib/tauri";

interface TunnelState {
  tunnels: CloudflareTunnel[];
  activeProcesses: Record<string, TunnelProcessState>;
  selectedTunnel: CloudflareTunnel | null;
  isLoading: boolean;
  error: string | null;

  fetchTunnels: (accountId: string) => Promise<void>;
  createTunnel: (accountId: string, name: string) => Promise<CloudflareTunnel | null>;
  startTunnel: (accountId: string, tunnelId: string) => Promise<void>;
  stopTunnel: (tunnelId: string) => Promise<void>;
  deleteTunnel: (accountId: string, tunnelId: string) => Promise<void>;
  refreshProcesses: () => Promise<void>;
  setSelectedTunnel: (tunnel: CloudflareTunnel | null) => void;
  updateTunnelProcessState: (tunnelId: string, status: "running" | "stopped") => void;
}

export const useTunnelStore = create<TunnelState>((set, get) => ({
  tunnels: [],
  activeProcesses: {},
  selectedTunnel: null,
  isLoading: false,
  error: null,

  fetchTunnels: async (accountId: string) => {
    try {
      set({ isLoading: true, error: null });
      const tunnels = await tauriApi.listTunnels(accountId);
      set({ tunnels });
      await get().refreshProcesses();
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  createTunnel: async (accountId: string, name: string) => {
    try {
      set({ isLoading: true, error: null });
      const tunnel = await tauriApi.createTunnel(accountId, name);
      await get().fetchTunnels(accountId);
      return tunnel;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return null;
    } finally {
      set({ isLoading: false });
    }
  },

  startTunnel: async (accountId: string, tunnelId: string) => {
    try {
      set({ isLoading: true, error: null });
      const process = await tauriApi.startTunnel(accountId, tunnelId);
      set((state) => ({
        activeProcesses: {
          ...state.activeProcesses,
          [tunnelId]: process,
        },
      }));
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  stopTunnel: async (tunnelId: string) => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.stopTunnel(tunnelId);
      set((state) => {
        const next = { ...state.activeProcesses };
        delete next[tunnelId];
        return { activeProcesses: next };
      });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  deleteTunnel: async (accountId: string, tunnelId: string) => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.deleteTunnel(accountId, tunnelId);
      await get().fetchTunnels(accountId);
      if (get().selectedTunnel?.id === tunnelId) {
        set({ selectedTunnel: null });
      }
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  refreshProcesses: async () => {
    try {
      const procs = await tauriApi.getActiveProcesses();
      const map: Record<string, TunnelProcessState> = {};
      for (const p of procs) {
        map[p.tunnel_id] = p;
      }
      set({ activeProcesses: map });
    } catch (err: unknown) {
      console.error("Failed to refresh processes:", err);
    }
  },

  setSelectedTunnel: (tunnel) => {
    set({ selectedTunnel: tunnel });
  },

  updateTunnelProcessState: (tunnelId, status) => {
    set((state) => {
      const next = { ...state.activeProcesses };
      if (status === "stopped") {
        delete next[tunnelId];
      }
      return { activeProcesses: next };
    });
  },
}));
