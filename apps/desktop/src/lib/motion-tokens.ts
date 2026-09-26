/**
 * Spring tokens: the single source for all motion (DESIGN §7).
 *
 * `visualDuration` and `bounce` follow SwiftUI's spring model and Motion's
 * `{ type: "spring", visualDuration, bounce }`. `scripts/gen-motion-css.ts` bakes the same
 * springs into CSS `linear()` easings in `styles/motion.css`, so simple transitions cost no JS.
 */
export const springs = {
  /** Toggles, segmented thumb, selection highlight, press release. */
  snappy: { visualDuration: 0.25, bounce: 0.1 },
  /** Disclosure, inspector open/close, list insert/remove, layout changes. */
  smooth: { visualDuration: 0.35, bounce: 0 },
  /** Sheets and dialogs. */
  sheet: { visualDuration: 0.45, bounce: 0 },
  /** Rare: success confirmation, copy tick. */
  bouncy: { visualDuration: 0.4, bounce: 0.2 },
  /** Direct manipulation: split-view drag, reorder, slider. */
  interactive: { visualDuration: 0.15, bounce: 0 },
} as const satisfies Record<string, SpringToken>;

export type SpringName = keyof typeof springs;

export interface SpringToken {
  readonly visualDuration: number;
  readonly bounce: number;
}

/** Physical parameters (mass 1), using the same conversion as Motion. */
export function springPhysics({ visualDuration, bounce }: SpringToken) {
  const root = (2 * Math.PI) / (visualDuration * 1.2);
  const stiffness = root * root;
  const dampingRatio = Math.min(1, Math.max(0.05, 1 - bounce));
  const damping = 2 * dampingRatio * Math.sqrt(stiffness);
  return { stiffness, damping, dampingRatio, angularFrequency: root };
}

/** Position (0 → 1) of a spring released from rest at time `t` seconds. */
export function springPosition(token: SpringToken, t: number): number {
  const { dampingRatio: z, angularFrequency: w } = springPhysics(token);
  if (z < 1) {
    const wd = w * Math.sqrt(1 - z * z);
    const envelope = Math.exp(-z * w * t);
    return 1 - envelope * (Math.cos(wd * t) + ((z * w) / wd) * Math.sin(wd * t));
  }
  return 1 - Math.exp(-w * t) * (1 + w * t);
}

const REST_DELTA = 0.001;
const STEP = 1 / 120;

/** Time (seconds) after which the spring stays within `REST_DELTA` of its target. */
export function springSettleTime(token: SpringToken): number {
  let lastOutside = 0;
  for (let t = 0; t <= 5; t += STEP) {
    if (Math.abs(1 - springPosition(token, t)) > REST_DELTA) lastOutside = t;
  }
  return lastOutside + STEP;
}

/** CSS `linear()` easing and duration approximating the spring. */
export function springToCss(token: SpringToken, samples = 40) {
  const duration = springSettleTime(token);
  const points: string[] = [];
  for (let i = 0; i <= samples; i++) {
    const value = i === samples ? 1 : springPosition(token, (duration * i) / samples);
    points.push(String(Math.round(value * 10000) / 10000));
  }
  return { easing: `linear(${points.join(", ")})`, durationMs: Math.round(duration * 1000) };
}

/** Motion (`motion/react`) transition for a token. */
export function spring(name: SpringName) {
  return { type: "spring", ...springs[name] } as const;
}
