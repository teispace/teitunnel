import { create } from "zustand";
import { tauriApi, type BinaryStatus, type DownloadProgress } from "@/lib/tauri";

interface BinaryStore {
  status: BinaryStatus | null;
  progress: DownloadProgress | null;
  isDownloading: boolean;
  error: string | null;

  checkStatus: () => Promise<void>;
  downloadManagedBinary: () => Promise<void>;
  setProgress: (progress: DownloadProgress) => void;
  setStatus: (status: BinaryStatus) => void;
}

export const useBinaryStore = create<BinaryStore>((set) => ({
  status: null,
  progress: null,
  isDownloading: false,
  error: null,

  checkStatus: async () => {
    try {
      const status = await tauriApi.checkBinaryStatus();
      set({ status, error: null });
    } catch (err: unknown) {
      set({ error: err instanceof Error ? err.message : String(err) });
    }
  },

  downloadManagedBinary: async () => {
    try {
      set({ isDownloading: true, error: null });
      const status = await tauriApi.downloadManagedBinary();
      set({ status, isDownloading: false, progress: null });
    } catch (err: unknown) {
      set({
        error: err instanceof Error ? err.message : String(err),
        isDownloading: false,
      });
    }
  },

  setProgress: (progress) => set({ progress }),
  setStatus: (status) => set({ status }),
}));
