import { useEffect, useLayoutEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { type Rgba, resolveColor, toRgba } from "@/lib/canvas-color";
import { cn } from "@/lib/cn";
import type { Columns } from "@/lib/traffic";
import { useAppearance } from "@/lib/use-appearance";

/** Semantic colours a series can use (DESIGN: tokens only). */
export type ChartTone = "accent" | "error" | "warning" | "secondary";

const TONES: Record<ChartTone | "axis" | "grid", string> = {
  accent: "var(--accent)",
  error: "var(--status-error)",
  warning: "var(--status-warning)",
  secondary: "var(--text-secondary)",
  axis: "var(--text-secondary)",
  grid: "var(--border-separator)",
};

type Palette = Record<keyof typeof TONES, Rgba>;

export interface ChartSeries {
  label: string;
  tone: ChartTone;
  /** Shade the area under the line. */
  fill?: boolean;
  /** Mostly empty (nulls): mark each value with a dot so lone samples stay visible. */
  sparse?: boolean;
  /** A value for the readout, e.g. "12/s". */
  format: (value: number | null) => string;
}

interface TimeSeriesChartProps {
  /** Accessible name, e.g. "Requests per second over the last hour". */
  label: string;
  /** x in seconds, then one column per series. */
  data: Columns;
  series: readonly ChartSeries[];
  /** Visible time window in seconds; defaults to the data's extent. */
  xRange?: readonly [from: number, to: number];
  /** Axis tick labels. */
  formatTick: (seconds: number) => string;
  /** The hovered time in the readout. */
  formatTime: (seconds: number) => string;
  /** Axis tick labels for values. */
  formatValue: (value: number) => string;
  /** The y axis never shows less than this, so an idle chart isn't all noise. */
  minMax?: number;
  height?: number;
  className?: string;
}

const FONT = "10px -apple-system, BlinkMacSystemFont, system-ui, sans-serif";

/** Index of the newest sample with a value in any series. */
function latestIndex(data: Columns): number | null {
  for (let i = data[0].length - 1; i >= 0; i--) {
    if (data.slice(1).some((column) => column[i] !== null && column[i] !== undefined)) return i;
  }
  return null;
}

/**
 * A time-series line chart on uPlot (canvas). Colours come from the design tokens and
 * repaint on appearance changes; nothing animates except new data arriving (DESIGN).
 * Hovering moves a crosshair, and the readout above shows the values under it, or the
 * latest values otherwise.
 */
export function TimeSeriesChart({
  label,
  data,
  series,
  xRange,
  formatTick,
  formatTime,
  formatValue,
  minMax = 1,
  height = 120,
  className,
}: TimeSeriesChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const palette = useRef<Palette | null>(null);
  const appearance = useAppearance();
  const [hovered, setHovered] = useState<number | null>(null);

  // Formatters live in a ref so the chart is built once, not on every render.
  const latest = useRef({ formatTick, formatValue, minMax, xRange, data });
  latest.current = { formatTick, formatValue, minMax, xRange, data };

  // biome-ignore lint/correctness/useExhaustiveDependencies: built once per series layout; data and window update below
  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    const colors = () => {
      palette.current ??= Object.fromEntries(
        Object.entries(TONES).map(([tone, css]) => [tone, resolveColor(el, css)]),
      ) as Palette;
      return palette.current;
    };
    const opts: uPlot.Options = {
      width: Math.max(el.clientWidth, 1),
      height,
      legend: { show: false },
      padding: [8, 4, 0, 0],
      cursor: {
        y: false,
        drag: { x: false, y: false, setScale: false },
        points: { size: 6, width: 0 },
      },
      scales: {
        x: {
          time: true,
          range: (_u, min, max) => {
            const w = latest.current.xRange;
            return w ? [w[0], w[1]] : [min, max];
          },
        },
        y: {
          range: (_u, _min, max) => [0, Math.max(latest.current.minMax, (max ?? 0) * 1.15)],
        },
      },
      axes: [
        {
          font: FONT,
          size: 20,
          gap: 4,
          space: 64,
          stroke: () => toRgba(colors().axis),
          grid: { show: false },
          ticks: { show: false },
          values: (_u, splits) => splits.map((s) => latest.current.formatTick(s)),
        },
        {
          font: FONT,
          size: 36,
          gap: 4,
          space: 28,
          stroke: () => toRgba(colors().axis),
          grid: { stroke: () => toRgba(colors().grid), width: 1 },
          ticks: { show: false },
          values: (_u, splits) => splits.map((s) => latest.current.formatValue(s)),
        },
      ],
      series: [
        {},
        ...series.map(
          (s): uPlot.Series => ({
            label: s.label,
            stroke: () => toRgba(colors()[s.tone]),
            ...(s.fill ? { fill: () => toRgba(colors()[s.tone], 0.14) } : {}),
            width: 1.5,
            spanGaps: false,
            points: {
              show: s.sparse === true,
              size: 4,
              width: 0,
              fill: () => toRgba(colors()[s.tone]),
            },
          }),
        ),
      ],
      hooks: {
        setCursor: [
          (u) => {
            const idx = u.cursor.idx;
            setHovered(idx === null || idx === undefined ? null : idx);
          },
        ],
      },
    };
    const chart = new uPlot(opts, latest.current.data as uPlot.AlignedData, el);
    plot.current = chart;
    // Resize on the next frame: resizing inside the callback would re-trigger layout in
    // the same frame (a "ResizeObserver loop").
    let frame = 0;
    const resize = new ResizeObserver(([entry]) => {
      const width = Math.round(entry?.contentRect.width ?? 0);
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (width > 0 && width !== chart.width) chart.setSize({ width, height });
      });
    });
    resize.observe(el);
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      chart.destroy();
      plot.current = null;
    };
  }, [height, series.map((s) => `${s.label}:${s.tone}:${s.fill}:${s.sparse}`).join("|")]);

  // New samples, or the window moved: redraw in place (no animation). setData re-runs
  // the x range function, which reads `xRange` from the ref.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the range bounds are triggers
  useEffect(() => {
    plot.current?.setData(data as uPlot.AlignedData);
  }, [data, xRange?.[0], xRange?.[1]]);

  // Colours changed: resolve the tokens again and repaint.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `appearance` is the trigger
  useEffect(() => {
    palette.current = null;
    plot.current?.redraw(false, true);
  }, [appearance]);

  const index = hovered ?? latestIndex(data);
  const time = hovered !== null ? data[0][hovered] : undefined;
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <div className="flex min-h-4 items-baseline gap-3 text-callout leading-4">
        {series.map((s, i) => (
          <span key={s.label} className="flex items-baseline gap-1.5">
            <span
              aria-hidden
              className={cn(
                "size-2 translate-y-[-1px] self-center rounded-full",
                s.tone === "accent" && "bg-accent",
                s.tone === "error" && "bg-error",
                s.tone === "warning" && "bg-warning",
                s.tone === "secondary" && "bg-secondary",
              )}
            />
            <span className="text-secondary">{s.label}</span>
            <span className="tabular">
              {s.format(index === null ? null : (data[i + 1]?.[index] ?? null))}
            </span>
          </span>
        ))}
        <span className="tabular ml-auto text-secondary">
          {time !== undefined ? formatTime(time) : ""}
        </span>
      </div>
      <div
        ref={host}
        role="img"
        aria-label={label}
        className="min-w-0 overflow-hidden"
        style={{ height }}
      />
    </div>
  );
}
