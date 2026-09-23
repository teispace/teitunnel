/** An sRGB colour with channels 0–255 and alpha 0–1. */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const TRANSPARENT: Rgba = { r: 0, g: 0, b: 0, a: 0 };

/**
 * Parses a computed CSS colour. WebKit serializes `light-dark()` and `color-mix()`
 * results as `rgb()`/`rgba()` or `color(srgb …)`; canvas doesn't reliably accept the
 * latter, so colours are normalized to numbers first.
 */
export function parseColor(value: string): Rgba {
  const text = value.trim();
  const rgb =
    /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/i.exec(text);
  if (rgb) {
    return {
      r: Number(rgb[1]),
      g: Number(rgb[2]),
      b: Number(rgb[3]),
      a: alpha(rgb[4]),
    };
  }
  const srgb =
    /^color\(\s*srgb\s+([\d.e-]+)\s+([\d.e-]+)\s+([\d.e-]+)(?:\s*\/\s*([\d.]+%?))?\s*\)$/i.exec(
      text,
    );
  if (srgb) {
    const channel = (v: string | undefined) =>
      Math.round(Math.min(1, Math.max(0, Number(v))) * 255);
    return { r: channel(srgb[1]), g: channel(srgb[2]), b: channel(srgb[3]), a: alpha(srgb[4]) };
  }
  return TRANSPARENT;
}

function alpha(value: string | undefined): number {
  if (value === undefined) return 1;
  return value.endsWith("%") ? Number(value.slice(0, -1)) / 100 : Number(value);
}

/** `color` as an `rgba()` string, its alpha multiplied by `opacity`. */
export function toRgba({ r, g, b, a }: Rgba, opacity = 1): string {
  const round = (n: number) => Math.round(n * 1000) / 1000;
  return `rgba(${r}, ${g}, ${b}, ${round(a * opacity)})`;
}

/** Resolves a CSS colour (e.g. `var(--accent)`) in `host`'s context. */
export function resolveColor(host: Element, css: string): Rgba {
  const probe = document.createElement("span");
  probe.style.color = css;
  probe.style.display = "none";
  host.appendChild(probe);
  const color = getComputedStyle(probe).color;
  probe.remove();
  return parseColor(color);
}
