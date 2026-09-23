import { useEffect, useMemo, useState } from "react";
import { LogViewer } from "@/components/patterns/log-viewer";
import { TimeSeriesChart } from "@/components/patterns/time-series-chart";
import type { LogLine } from "@/lib/ipc/bindings";
import { LIVE_CAPACITY, withGaps } from "@/lib/traffic";

/** Frame intervals (ms) since the page mounted, read by the perf script. */
declare global {
  interface Window {
    __stress?: { frames: number[]; logLines: number; chartUpdates: number };
  }
}

const LOG_BATCH = 200; // every 100 ms: 2,000 lines/s
const LOG_WINDOW = 1_000; // what the backend returns at most
const CHART_EVERY_MS = 50; // 20 Hz: 20× the live rate

/**
 * A stress bench for the M5 exit criteria (D-051): the log viewer under 2,000 lines/s
 * and a 3,600-point chart updating at 20 Hz, while every frame's interval is recorded.
 */
export default function Stress() {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [series, setSeries] = useState<{ at: number[]; rate: number[] }>({ at: [], rate: [] });

  useEffect(() => {
    const stats = { frames: [] as number[], logLines: 0, chartUpdates: 0 };
    window.__stress = stats;
    let last = performance.now();
    let frame = requestAnimationFrame(function tick(now) {
      stats.frames.push(now - last);
      last = now;
      frame = requestAnimationFrame(tick);
    });
    let n = 0;
    const logs = setInterval(() => {
      const batch = Array.from({ length: LOG_BATCH }, (): LogLine => {
        n += 1;
        return n % 50 === 0
          ? {
              time: null,
              level: "error",
              message: `Request failed #${n}`,
              error: "dial tcp 127.0.0.1:3000: connect: connection refused",
            }
          : {
              time: null,
              level: "info",
              message: `GET https://app.xyz.com/items/${n} HTTP/1.1 connIndex=${n % 4}`,
              error: null,
            };
      });
      stats.logLines += batch.length;
      setLines((prev) => [...prev, ...batch].slice(-LOG_WINDOW));
    }, 100);
    let t = Date.now() - LIVE_CAPACITY * 1000;
    const chart = setInterval(() => {
      t += 1000;
      stats.chartUpdates += 1;
      setSeries((prev) => ({
        at: [...prev.at, t].slice(-LIVE_CAPACITY),
        rate: [...prev.rate, 10 + 8 * Math.sin(t / 60_000) + Math.random() * 3].slice(
          -LIVE_CAPACITY,
        ),
      }));
    }, CHART_EVERY_MS);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(logs);
      clearInterval(chart);
    };
  }, []);

  const data = useMemo(() => withGaps(series.at, [series.rate], 25), [series]);
  const end = (series.at.at(-1) ?? Date.now()) / 1000;
  return (
    <div className="flex flex-col gap-6 overflow-y-auto p-5">
      <h1 className="text-title2">Stress</h1>
      <TimeSeriesChart
        label="Requests per second"
        data={data}
        series={[
          {
            label: "Requests",
            tone: "accent",
            fill: true,
            format: (v) => `${v?.toFixed(1) ?? "–"}/s`,
          },
        ]}
        xRange={[end - 3_600, end]}
        formatTick={(s) =>
          new Date(s * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
        }
        formatTime={(s) => new Date(s * 1000).toLocaleTimeString()}
        formatValue={(v) => v.toFixed(0)}
      />
      <LogViewer lines={lines} empty="Waiting…" height={320} />
    </div>
  );
}
