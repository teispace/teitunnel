import { create } from "zustand";
import {
  tauriApi,
  type DnsRecord,
  type DnsHygieneReport,
} from "@/lib/tauri";

interface DnsStore {
  records: DnsRecord[];
  hygieneReport: DnsHygieneReport | null;
  isLoading: boolean;
  isScanning: boolean;
  isCleaning: boolean;
  error: string | null;

  fetchRecords: (zoneId: string) => Promise<void>;
  createCname: (zoneId: string, name: string, tunnelUuid: string) => Promise<boolean>;
  deleteRecord: (zoneId: string, recordId: string) => Promise<boolean>;
  scanHygiene: (accountId: string, zoneId: string) => Promise<void>;
  cleanupAllOrphaned: (zoneId: string) => Promise<number>;
}

export const useDnsStore = create<DnsStore>((set, get) => ({
  records: [],
  hygieneReport: null,
  isLoading: false,
  isScanning: false,
  isCleaning: false,
  error: null,

  fetchRecords: async (zoneId: string) => {
    try {
      set({ isLoading: true, error: null });
      const records = await tauriApi.listDnsRecords(zoneId, "CNAME");
      set({ records });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isLoading: false });
    }
  },

  createCname: async (zoneId: string, name: string, tunnelUuid: string) => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.createDnsCname(zoneId, name, tunnelUuid);
      await get().fetchRecords(zoneId);
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  deleteRecord: async (zoneId: string, recordId: string) => {
    try {
      set({ isLoading: true, error: null });
      await tauriApi.deleteDnsRecord(zoneId, recordId);
      await get().fetchRecords(zoneId);
      return true;
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
      return false;
    } finally {
      set({ isLoading: false });
    }
  },

  scanHygiene: async (accountId: string, zoneId: string) => {
    try {
      set({ isScanning: true, error: null });
      const report = await tauriApi.scanDnsHygiene(accountId, zoneId);
      set({ hygieneReport: report });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ isScanning: false });
    }
  },

  cleanupAllOrphaned: async (zoneId: string) => {
    const { hygieneReport } = get();
    if (!hygieneReport || hygieneReport.orphaned_records.length === 0) return 0;

    try {
      set({ isCleaning: true, error: null });
      let count = 0;
      for (const orphaned of hygieneReport.orphaned_records) {
        try {
          await tauriApi.deleteDnsRecord(zoneId, orphaned.record.id);
          count++;
        } catch (e) {
          console.error("Failed to delete record:", orphaned.record.id, e);
        }
      }
      // Re-fetch
      await get().fetchRecords(zoneId);
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
