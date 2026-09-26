import { useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { commands, type ExchangeId, type LiveBatch } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { fetchExchanges, inspectorKeys } from "./queries";
import { ExchangeStore, MAX_ROWS } from "./store";

/** Requests read per page (the inspector's maximum). */
export const PAGE = 1000;

/** How long a search waits after new traffic before it's run again. */
const SEARCH_REFRESH_MS = 600;

export interface LiveExchanges {
  store: ExchangeStore;
  status: "loading" | "ready" | "error";
  error: string | null;
  /** More (older) requests can be read. */
  hasOlder: boolean;
  loadingOlder: boolean;
  loadOlder: () => void;
  paused: boolean;
  setPaused: (paused: boolean) => void;
  /** New requests that arrived while paused. */
  waiting: number;
  /** Reads the list again (after a failure). */
  reload: () => void;
}

/**
 * The requests of one tap (or all), newest first: a page read from the inspector, then
 * kept current from `inspect_subscribe` batches (at most every 100 ms) until unmount,
 * which unsubscribes. A search runs in the inspector (it looks into bodies), so while one
 * is active new traffic re-runs it instead of being matched here. Pausing holds batches
 * back until resumed.
 */
export function useLiveExchanges(
  tap: string | null,
  search: string,
  /** Read and keep only the newest this many (a glance, not the list). */
  limit?: number,
): LiveExchanges {
  const queryClient = useQueryClient();
  const page = limit ?? PAGE;
  const store = useMemo(() => new ExchangeStore(tap, limit ?? MAX_ROWS), [tap, limit]);
  const [status, setStatus] = useState<LiveExchanges["status"]>("loading");
  const [error, setError] = useState<string | null>(null);
  const [older, setOlder] = useState<ExchangeId | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [paused, setPausedState] = useState(false);
  const [waiting, setWaiting] = useState(0);
  const pausedRef = useRef(false);
  const held = useRef<LiveBatch[]>([]);
  const text = search.trim();

  // Read the first page, and again when the list must be rebuilt (lagged, a search).
  const generation = useRef(0);
  const load = useCallback(async () => {
    const mine = ++generation.current;
    try {
      const read = await fetchExchanges({ tap, text: text || null, limit: page });
      if (mine !== generation.current) return;
      store.reset(read.items);
      // A glance never reads further back.
      setOlder(limit === undefined ? read.next : null);
      setStatus("ready");
      setError(null);
    } catch (failure) {
      if (mine !== generation.current) return;
      setStatus("error");
      setError(toIpcError(failure).message);
    }
  }, [store, tap, text, page, limit]);

  useEffect(() => {
    let cancelled = false;
    let subscription: number | null = null;
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    let loaded = false;
    const early: LiveBatch[] = [];
    held.current = [];
    setWaiting(0);
    setStatus("loading");

    const scheduleSearch = () => {
      clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => void load(), SEARCH_REFRESH_MS);
    };

    const apply = (batch: LiveBatch) => {
      if (batch.tapsChanged) {
        void queryClient.invalidateQueries({ queryKey: inspectorKeys.taps() });
        void queryClient.invalidateQueries({ queryKey: inspectorKeys.known() });
      }
      if (batch.lagged) {
        void load();
        return;
      }
      // A search can't be matched here: known rows update, new ones re-run it.
      const added = store.apply(batch, () => !text);
      if (text && batch.exchanges.length > added && batch.exchanges.length > 0) scheduleSearch();
    };

    const onBatch = (batch: LiveBatch) => {
      if (cancelled) return;
      if (!loaded) {
        early.push(batch);
        return;
      }
      if (pausedRef.current) {
        held.current.push(batch);
        setWaiting((count) => count + batch.exchanges.length);
        return;
      }
      apply(batch);
    };

    const channel = new Channel<LiveBatch>();
    channel.onmessage = onBatch;
    commands
      .inspectSubscribe(channel)
      .then((id) => {
        if (cancelled) void commands.inspectUnsubscribe(id);
        else subscription = id;
      })
      .catch((failure: unknown) => {
        if (cancelled) return;
        setStatus("error");
        setError(toIpcError(failure).message);
      });
    void load().then(() => {
      loaded = true;
      for (const batch of early.splice(0)) onBatch(batch);
    });

    return () => {
      cancelled = true;
      clearTimeout(refreshTimer);
      generation.current++;
      if (subscription !== null) void commands.inspectUnsubscribe(subscription);
    };
  }, [store, load, text, queryClient]);

  const setPaused = useCallback(
    (next: boolean) => {
      pausedRef.current = next;
      setPausedState(next);
      if (next) return;
      const batches = held.current.splice(0);
      setWaiting(0);
      for (const batch of batches) {
        if (batch.lagged) {
          void load();
          return;
        }
        store.apply(batch, () => !text);
      }
      if (text && batches.length > 0) void load();
    },
    [store, load, text],
  );

  const loadOlder = useCallback(() => {
    if (!older || loadingOlder) return;
    setLoadingOlder(true);
    fetchExchanges({ tap, text: text || null, limit: PAGE, before: older })
      .then((page) => {
        store.appendOlder(page.items);
        setOlder(page.next);
      })
      .catch((failure: unknown) => setError(toIpcError(failure).message))
      .finally(() => setLoadingOlder(false));
  }, [older, loadingOlder, store, tap, text]);

  return {
    store,
    status,
    error,
    hasOlder: older !== null,
    loadingOlder,
    loadOlder,
    paused,
    setPaused,
    waiting,
    reload: () => void load(),
  };
}

/** The store's rows, re-rendering when they change. */
export function useRows(store: ExchangeStore) {
  return useSyncExternalStore(store.subscribe, store.rows, store.rows);
}
