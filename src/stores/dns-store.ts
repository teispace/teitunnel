import { create } from "zustand";
import {
  tauriApi,
  type DnsRecord,
  type DnsHygieneReport,
} from "@/lib/tauri";
import { useAuthStore } from "@/stores/auth-store";

interface DnsStore {
  records: DnsRecord[];
  hygieneReport: DnsHygieneReport | null;
  isLoading: boolean;
  isScanning: boolean;
  isCleaning: boolean;
  error: string | null;

  fetchRecords: (zoneId: string, token?: string) => Promise<void>;
  createCname: (zoneId: string, name: string, tunnelUuid: string, token?: string) => Promise<boolean>;
  deleteRecord: (zoneId: string, recordId: string, token?: string) => Promise<boolean>;
  scanHygiene: (accountId: string, zoneId: string, token?: string) => Promise<void>;
  cleanupAllOrphaned: (zoneId: string, token?: string) => Promise<number>;
}

export const useDnsStore = create<DnsStore>((set, get) => ({
  records: [],
  hygieneReport: null,
  isLoading: false,
  isScanning: false,
  isCleaning: false,
  error: null,

  fetchRecords: async (zoneId: string, token?: string) => {
    try {
      set({ isLoading: true, error: null });
      const effectiveToken = token || useAuthStore.getState().token || undefined;
      const records = await tauriApi.listDnsRecords(zoneId, "CNAME", effectiveToken);
      set({ records });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  createCname: async (zoneId: string, name: string, tunnelUuid: string, token?: string) => {
    try {
      set({ isLoading: true, error: null });
      const effectiveToken = token || useAuthStore.getState().token || undefined;
      await tauriApi.createDnsCname(zoneId, name, tunnelUuid, effectiveToken);
      await get().fetchRecords(zoneId, effectiveToken);
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  deleteRecord: async (zoneId: string, recordId: string, token?: string) => {
    try {
      set({ isLoading: true, error: null });
      const effectiveToken = token || useAuthStore.getState().token || undefined;
      await tauriApi.deleteDnsRecord(zoneId, recordId, effectiveToken);
      await get().fetchRecords(zoneId, effectiveToken);
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  scanHygiene: async (accountId: string, zoneId: string, token?: string) => {
    try {
      set({ isScanning: true, error: null });
      const effectiveToken = token || useAuthStore.getState().token || undefined;
      const report = await tauriApi.scanDnsHygiene(accountId, zoneId, effectiveToken);
      set({ hygieneReport: report });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isScanning: false });
    }
  },

  cleanupAllOrphaned: async (zoneId: string, token?: string) => {
    const { hygieneReport } = get();
    if (!hygieneReport || hygieneReport.orphaned_records.length === 0) return 0;

    try {
      set({ isCleaning: true, error: null });
      const effectiveToken = token || useAuthStore.getState().token || undefined;
      let count = 0;
      for (const orphaned of hygieneReport.orphaned_records) {
        try {
          await tauriApi.deleteDnsRecord(zoneId, orphaned.record.id, effectiveToken);
          count++;
        } catch (e) {
          console.error("Failed to delete record:", orphaned.record.id, e);
        }
      }
      // Re-fetch
      await get().fetchRecords(zoneId, effectiveToken);
      set((s) => ({
        hygieneReport: s.hygieneReport
          ? {
              ...s.hygieneReport,
              orphaned_records: [],
            }
          : null,
      }));
      return count;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return 0;
    } finally {
      set({ isCleaning: false });
    }
  },
}));
