import { useId } from "react";

interface SparklineProps {
  values: readonly number[];
  /** Accessible summary, e.g. "Requests over the last hour". */
  label: string;
  height?: number;
}

/**
 * A small area chart. SVG with a stretched viewBox so it fills its container; no axes
 * and no animation (DESIGN: charts only move when data arrives).
 */
export function Sparkline({ values, label, height = 36 }: SparklineProps) {
  const gradient = useId();
  const width = Math.max(values.length - 1, 1);
  const max = Math.max(1, ...values);
  const points = values.map((v, i) => `${i},${height - (v / max) * (height - 2) - 1}`);
  const line = points.length > 0 ? `M${points.join(" L")}` : "";
  const area = points.length > 0 ? `${line} L${width},${height} L0,${height} Z` : "";
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className="block w-full text-accent"
      style={{ height }}
    >
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="currentColor" stopOpacity="0.22" />
          <stop offset="1" stopColor="currentColor" stopOpacity="0" />
        </linearGradient>
      </defs>
      {values.length > 1 ? (
        <>
          <path d={area} fill={`url(#${gradient})`} />
          <path
            d={line}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            vectorEffect="non-scaling-stroke"
            strokeLinejoin="round"
          />
        </>
      ) : null}
    </svg>
  );
}
