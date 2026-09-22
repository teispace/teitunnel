import { create } from "zustand";
import { tauriApi, type CloudflareAccount, type CloudflareZone, type CertStatus } from "@/lib/tauri";

interface AuthState {
  token: string | null;
  accounts: CloudflareAccount[];
  activeAccountId: string | null;
  zones: CloudflareZone[];
  activeZoneId: string | null;
  certStatus: CertStatus | null;
  isLoggingInBrowser: boolean;
  browserLoginUrl: string | null;
  isLoading: boolean;
  error: string | null;

  initAuth: () => Promise<void>;
  checkCert: () => Promise<void>;
  startBrowserLogin: () => Promise<void>;
  cancelBrowserLogin: () => Promise<void>;
  logoutCert: () => Promise<void>;
  setBrowserLoginUrl: (url: string | null) => void;
  setBrowserLoginSuccess: () => void;
  setBrowserLoginFailed: (err: string) => void;
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
  certStatus: null,
  isLoggingInBrowser: false,
  browserLoginUrl: null,
  isLoading: false,
  error: null,

  initAuth: async () => {
    try {
      set({ isLoading: true, error: null });

      // 1. Check cert.pem from browser login
      let cert: CertStatus | null = null;
      try {
        cert = await tauriApi.checkCertStatus();
        set({ certStatus: cert });
      } catch {
        // ignore if check fails
      }

      // 2. Check saved API token (from Keychain)
      let effectiveToken = await tauriApi.getSavedToken();
      // If no manual token was configured, automatically use the token from cert.pem!
      if (!effectiveToken && cert?.api_token) {
        effectiveToken = cert.api_token;
      }

      if (effectiveToken) {
        set({ token: effectiveToken });
        try {
          const accounts = await tauriApi.listAccounts(effectiveToken);
          set({ accounts });
          if (accounts.length > 0) {
            const firstAccount =
              cert?.account_id && accounts.some((a) => a.id === cert.account_id)
                ? cert.account_id
                : accounts[0].id;
            set({ activeAccountId: firstAccount });
            const zones = await tauriApi.listZones(firstAccount, effectiveToken);
            set({
              zones,
              activeZoneId:
                cert?.zone_id && zones.some((z) => z.id === cert.zone_id)
                  ? cert.zone_id
                  : zones[0]?.id || null,
            });
          }
        } catch (apiErr) {
          console.error("Failed to list accounts with token:", apiErr);
          if (cert?.zone_id && cert?.account_id) {
            set({
              activeAccountId: cert.account_id,
              activeZoneId: cert.zone_id,
            });
            try {
              const directZones = await tauriApi.listZones(cert.account_id, effectiveToken);
              set({ zones: directZones });
            } catch {
              // fallback
            }
          }
        }
      }
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  checkCert: async () => {
    try {
      const cert = await tauriApi.checkCertStatus();
      set({ certStatus: cert });
      if (cert.has_cert && cert.api_token) {
        const { token } = get();
        if (!token) {
          await get().initAuth();
        }
      }
    } catch (err: unknown) {
      console.error("Failed to check cert:", err);
    }
  },

  startBrowserLogin: async () => {
    try {
      set({ isLoggingInBrowser: true, browserLoginUrl: null, error: null });
      await tauriApi.startBrowserLogin();
    } catch (err: unknown) {
      set({
        isLoggingInBrowser: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  cancelBrowserLogin: async () => {
    try {
      await tauriApi.cancelBrowserLogin();
    } finally {
      set({ isLoggingInBrowser: false, browserLoginUrl: null });
    }
  },

  logoutCert: async () => {
    try {
      await tauriApi.deleteCert();
      const savedToken = await tauriApi.getSavedToken();
      set({
        certStatus: {
          has_cert: false,
          cert_path: null,
          zone_id: null,
          account_id: null,
          api_token: null,
        },
        token: savedToken || null,
      });
      if (!savedToken) {
        set({
          accounts: [],
          activeAccountId: null,
          zones: [],
          activeZoneId: null,
        });
      }
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    }
  },

  setBrowserLoginUrl: (url: string | null) => {
    set({ browserLoginUrl: url });
  },

  setBrowserLoginSuccess: () => {
    set({ isLoggingInBrowser: false, browserLoginUrl: null });
    get().checkCert();
  },

  setBrowserLoginFailed: (err: string) => {
    set({ isLoggingInBrowser: false, browserLoginUrl: null, error: err });
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
