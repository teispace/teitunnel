import { create } from "zustand";
import { tauriApi, type CloudflareAccount, type CloudflareZone } from "@/lib/tauri";

interface AuthState {
  token: string | null;
  accounts: CloudflareAccount[];
  activeAccountId: string | null;
  zones: CloudflareZone[];
  activeZoneId: string | null;
  isLoading: boolean;
  error: string | null;

  initAuth: () => Promise<void>;
  saveToken: (token: string) => Promise<boolean>;
  logout: () => Promise<void>;
  selectAccount: (accountId: string) => Promise<void>;
  selectZone: (zoneId: string) => void;
  refreshAccounts: () => Promise<void>;
  refreshZones: (accountId?: string) => Promise<void>;
}

export const useAuthStore = create<AuthState>((set, get) => ({
  token: null,
  accounts: [],
  activeAccountId: null,
  zones: [],
  activeZoneId: null,
  isLoading: false,
  error: null,

  initAuth: async () => {
    try {
      set({ isLoading: true, error: null });
      const savedToken = await tauriApi.getSavedToken();
      if (savedToken) {
        set({ token: savedToken });
        const accounts = await tauriApi.listAccounts(savedToken);
        set({ accounts });
        if (accounts.length > 0) {
          const firstAccount = accounts[0].id;
          set({ activeAccountId: firstAccount });
          const zones = await tauriApi.listZones(firstAccount, savedToken);
          set({ zones, activeZoneId: zones[0]?.id || null });
        }
      }
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  saveToken: async (token: string) => {
    try {
      set({ isLoading: true, error: null });
      const isValid = await tauriApi.verifyAndSaveToken(token);
      if (isValid) {
        set({ token });
        const accounts = await tauriApi.listAccounts(token);
        set({ accounts });
        if (accounts.length > 0) {
          const firstAccount = accounts[0].id;
          set({ activeAccountId: firstAccount });
          const zones = await tauriApi.listZones(firstAccount, token);
          set({ zones, activeZoneId: zones[0]?.id || null });
        }
        return true;
      } else {
        set({ error: "Invalid Cloudflare API token. Please check permissions." });
        return false;
      }
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  logout: async () => {
    await tauriApi.deleteSavedToken();
    set({
      token: null,
      accounts: [],
      activeAccountId: null,
      zones: [],
      activeZoneId: null,
      error: null,
    });
  },

  selectAccount: async (accountId: string) => {
    set({ activeAccountId: accountId, isLoading: true });
    try {
      const { token } = get();
      const zones = await tauriApi.listZones(accountId, token || undefined);
      set({ zones, activeZoneId: zones[0]?.id || null });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  selectZone: (zoneId: string) => {
    set({ activeZoneId: zoneId });
  },

  refreshAccounts: async () => {
    const { token } = get();
    if (!token) return;
    try {
      const accounts = await tauriApi.listAccounts(token);
      set({ accounts });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    }
  },

  refreshZones: async (accountId?: string) => {
    const { token, activeAccountId } = get();
    const acc = accountId || activeAccountId;
    if (!token || !acc) return;
    try {
      const zones = await tauriApi.listZones(acc, token);
      set({ zones });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    }
  },
}));
