import { create } from "zustand";
import {
  tauriApi,
  type CloudflareTunnel,
  type TunnelProcessState,
  type DirectTunnel,
} from "@/lib/tauri";

const DIRECT_TUNNELS_STORAGE_KEY = "teitunnel_direct_tunnels";

function loadSavedDirectTunnels(): DirectTunnel[] {
  try {
    const raw = localStorage.getItem(DIRECT_TUNNELS_STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function persistDirectTunnels(list: DirectTunnel[]) {
  try {
    localStorage.setItem(DIRECT_TUNNELS_STORAGE_KEY, JSON.stringify(list));
  } catch (e) {
    console.error("Failed to persist direct tunnels:", e);
  }
}

interface TunnelState {
  tunnels: CloudflareTunnel[];
  directTunnels: DirectTunnel[];
  activeProcesses: Record<string, TunnelProcessState>;
  selectedTunnel: CloudflareTunnel | null;
  isLoading: boolean;
  error: string | null;

  initTunnels: () => Promise<void>;
  fetchTunnels: (accountId: string, token?: string) => Promise<void>;
  fetchCertTunnels: () => Promise<void>;
  createTunnel: (accountId: string, name: string) => Promise<CloudflareTunnel | null>;
  createCertTunnel: (name: string) => Promise<CloudflareTunnel | null>;
  startTunnel: (accountId: string, tunnelId: string, token?: string) => Promise<boolean>;
  startNamedTunnel: (tunnelName: string) => Promise<boolean>;
  startDirectTunnel: (tunnelId: string, token: string) => Promise<boolean>;
  stopTunnel: (tunnelId: string) => Promise<boolean>;
  deleteTunnel: (accountId: string, tunnelId: string, token?: string) => Promise<boolean>;
  deleteCertTunnel: (tunnelId: string) => Promise<boolean>;
  saveDirectTunnel: (name: string, token: string) => DirectTunnel;
  deleteDirectTunnel: (tunnelId: string) => Promise<void>;
  refreshProcesses: () => Promise<void>;
  setSelectedTunnel: (tunnel: CloudflareTunnel | null) => void;
  updateTunnelProcessState: (tunnelId: string, status: "running" | "stopped") => void;
}

export const useTunnelStore = create<TunnelState>((set, get) => ({
  tunnels: [],
  directTunnels: loadSavedDirectTunnels(),
  activeProcesses: {},
  selectedTunnel: null,
  isLoading: false,
  error: null,

  initTunnels: async () => {
    set({ directTunnels: loadSavedDirectTunnels() });
    await get().refreshProcesses();
  },

  fetchTunnels: async (accountId: string, token?: string) => {
    try {
      set({ isLoading: true, error: null });
      const tunnels = await tauriApi.listTunnels(accountId, token);
      set({ tunnels: tunnels || [] });
      await get().refreshProcesses();
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  fetchCertTunnels: async () => {
    try {
      set({ isLoading: true, error: null });
      const tunnels = await tauriApi.listCertTunnels();
      set({ tunnels: tunnels || [] });
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

  createCertTunnel: async (name: string) => {
    try {
      set({ isLoading: true, error: null });
      const tunnel = await tauriApi.createCertTunnel(name);
      await get().fetchCertTunnels();
      return tunnel;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return null;
    } finally {
      set({ isLoading: false });
    }
  },

  startTunnel: async (accountId: string, tunnelId: string, token?: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      const process = await tauriApi.startTunnel(accountId, tunnelId, token);
      set((state) => ({
        activeProcesses: {
          ...state.activeProcesses,
          [tunnelId]: process,
        },
      }));
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  startNamedTunnel: async (tunnelName: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      const process = await tauriApi.startNamedTunnel(tunnelName);
      set((state) => ({
        activeProcesses: {
          ...state.activeProcesses,
          [tunnelName]: process,
        },
      }));
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  startDirectTunnel: async (tunnelId: string, token: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      const process = await tauriApi.startTunnelByToken(tunnelId, token);
      set((state) => ({
        activeProcesses: {
          ...state.activeProcesses,
          [tunnelId]: process,
        },
      }));
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  stopTunnel: async (tunnelId: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.stopTunnel(tunnelId);
      set((state) => {
        const next = { ...state.activeProcesses };
        delete next[tunnelId];
        return { activeProcesses: next };
      });
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  deleteTunnel: async (accountId: string, tunnelId: string, token?: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.deleteTunnel(accountId, tunnelId, token);
      // Remove immediately for smooth UI feedback
      set((state) => ({
        tunnels: state.tunnels.filter((t) => t.id !== tunnelId),
        selectedTunnel: state.selectedTunnel?.id === tunnelId ? null : state.selectedTunnel,
      }));
      if (accountId) {
        await get().fetchTunnels(accountId, token).catch(() => {});
      }
      await get().fetchCertTunnels().catch(() => {});
      await get().refreshProcesses();
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  deleteCertTunnel: async (tunnelId: string): Promise<boolean> => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.deleteCertTunnel(tunnelId);
      set((state) => ({
        tunnels: state.tunnels.filter((t) => t.id !== tunnelId),
        selectedTunnel: state.selectedTunnel?.id === tunnelId ? null : state.selectedTunnel,
      }));
      await get().fetchCertTunnels().catch(() => {});
      await get().refreshProcesses();
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  saveDirectTunnel: (name: string, token: string) => {
    const newTunnel: DirectTunnel = {
      id: "dt-" + Date.now().toString(36) + Math.random().toString(36).substring(2, 6),
      name: name.trim() || "Zero Trust Tunnel",
      token: token.trim(),
      createdAt: new Date().toISOString(),
    };
    const updated = [newTunnel, ...get().directTunnels];
    set({ directTunnels: updated });
    persistDirectTunnels(updated);
    return newTunnel;
  },

  deleteDirectTunnel: async (tunnelId: string) => {
    await get().stopTunnel(tunnelId);
    const updated = get().directTunnels.filter((t) => t.id !== tunnelId);
    set({ directTunnels: updated });
    persistDirectTunnels(updated);
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
