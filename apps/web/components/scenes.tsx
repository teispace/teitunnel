"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";

/**
 * Scroll scenes: every `[data-scene]` element gets `--p`, its progress from 0 to 1, which
 * CSS turns into movement (transforms and opacity only). Kinds:
 *
 * - `view`: 0 when its top reaches the bottom of the window, 1 when its bottom leaves the top.
 * - `enter`: 0 when its top reaches the bottom, 1 when its top reaches `data-end` of the window
 *   (a fraction, 0.35 by default): things that settle into place as they arrive.
 * - `pin`: a tall section whose `sticky` child stays on screen; 0 when the section's top
 *   reaches the top of the window, 1 when its bottom reaches the bottom.
 * - `stack`: a sticky card in a stack; 0 until the next card starts covering it, 1 once the
 *   next card has reached the same place (the last card stays at 0).
 *
 * `data-steps="5"` also sets `data-step` (0–4) for discrete states, and a `[data-track]` inside a
 * scene is shifted sideways by `--x` so it ends flush right at 1 (horizontal galleries).
 * `[data-spotlight]` elements follow the pointer with `--mx` and `--my`.
 *
 * Only scenes near the window are computed, once per frame at most. Without JavaScript, or with
 * Reduce Motion (no `html.tt-motion`), nothing moves: the CSS shows every scene at rest.
 */

type Listener = (progress: number) => void;

interface Scene {
  element: HTMLElement;
  kind: string;
  end: number;
  steps: number;
  track: HTMLElement | null;
  /** How far the track moves: its width beyond its window's content box. */
  room: number;
  /** `stack`: the card that covers this one, and where that one sticks. */
  next: HTMLElement | null;
  stickTop: number;
  progress: number;
  step: number;
}

const listeners = new Map<Element, Set<Listener>>();

/** Calls `listener` with `element`'s progress whenever it changes (for scenes drawn in JS). */
export function onSceneProgress(element: Element, listener: Listener): () => void {
  const set = listeners.get(element) ?? new Set<Listener>();
  set.add(listener);
  listeners.set(element, set);
  return () => {
    set.delete(listener);
    if (set.size === 0) listeners.delete(element);
  };
}

const clamp = (value: number) => (value < 0 ? 0 : value > 1 ? 1 : value);

function measure(scene: Scene, height: number): number {
  if (scene.kind === "stack") {
    if (!scene.next) return 0;
    const top = scene.next.getBoundingClientRect().top;
    return clamp((height - top) / Math.max(1, height - scene.stickTop));
  }
  const rect = scene.element.getBoundingClientRect();
  switch (scene.kind) {
    case "pin": {
      const room = rect.height - height;
      return room <= 0 ? clamp(-rect.top / height) : clamp(-rect.top / room);
    }
    case "enter":
      return clamp((height - rect.top) / (height * (1 - scene.end)));
    default:
      return clamp((height - rect.top) / (height + rect.height));
  }
}

function apply(scene: Scene, progress: number) {
  if (Math.abs(progress - scene.progress) < 0.0005 && scene.progress !== -1) return;
  scene.progress = progress;
  const { element, steps, track } = scene;
  element.style.setProperty("--p", progress.toFixed(4));
  if (steps > 1) {
    const step = Math.min(steps - 1, Math.floor(progress * steps));
    if (step !== scene.step) {
      scene.step = step;
      element.dataset.step = String(step);
    }
  }
  if (track) element.style.setProperty("--x", `${(-scene.room * progress).toFixed(1)}px`);
  for (const listener of listeners.get(element) ?? []) listener(progress);
}

/** How far `track` must move for its end to meet its window's padding. */
function roomOf(track: HTMLElement | null): number {
  const window = track?.parentElement;
  if (!track || !window) return 0;
  const style = getComputedStyle(window);
  const inner =
    window.clientWidth -
    Number.parseFloat(style.paddingLeft) -
    Number.parseFloat(style.paddingRight);
  return Math.max(0, track.scrollWidth - inner);
}

export function Scenes() {
  const pathname = usePathname();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new page brings new scenes
  useEffect(() => {
    const root = document.documentElement;
    const elements = [...document.querySelectorAll<HTMLElement>("[data-scene]")];
    const scenes = elements.map(
      (element): Scene => ({
        element,
        kind: element.dataset.scene ?? "view",
        end: Number(element.dataset.end ?? 0.35),
        steps: Number(element.dataset.steps ?? 0),
        track: element.querySelector<HTMLElement>("[data-track]"),
        room: 0,
        next:
          element.dataset.scene === "stack"
            ? (element.nextElementSibling as HTMLElement | null)
            : null,
        stickTop: 0,
        progress: -1,
        step: -1,
      }),
    );
    const moving = () => root.classList.contains("tt-motion");
    if (!moving() || !("IntersectionObserver" in window)) {
      // At rest: finished, except stacked cards, which stay uncovered.
      for (const scene of scenes) apply(scene, scene.kind === "stack" ? 0 : 1);
      return;
    }

    for (const scene of scenes) scene.room = roomOf(scene.track);
    const near = new Set<Scene>();
    const byElement = new Map(scenes.map((scene) => [scene.element, scene]));
    let frame = 0;
    const update = () => {
      frame = 0;
      const height = window.innerHeight;
      for (const scene of near) apply(scene, measure(scene, height));
    };
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(update);
    };
    // Stacked cards stick according to their height (`--h`), and a card is covered once
    // the next one reaches the place where it sticks.
    const stacks = scenes.filter((scene) => scene.kind === "stack");
    const measureStacks = () => {
      for (const scene of stacks) {
        scene.element.style.setProperty("--h", `${scene.element.offsetHeight}px`);
      }
      for (const scene of stacks) {
        scene.stickTop = scene.next ? Number.parseFloat(getComputedStyle(scene.next).top) || 0 : 0;
      }
    };
    const onResize = () => {
      measureStacks();
      for (const scene of scenes) {
        scene.room = roomOf(scene.track);
        scene.progress = -1;
      }
      schedule();
    };
    // Cards change height as their screenshots load and as the window narrows.
    const heights = new ResizeObserver(onResize);
    for (const scene of stacks) heights.observe(scene.element);
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const scene = byElement.get(entry.target as HTMLElement);
          if (!scene) continue;
          if (entry.isIntersecting) near.add(scene);
          else {
            // Leaving: settle at the end it left by, so nothing is stuck halfway.
            apply(scene, measure(scene, window.innerHeight));
            near.delete(scene);
          }
        }
        schedule();
      },
      { rootMargin: "25% 0px 25% 0px" },
    );
    for (const scene of scenes) {
      apply(scene, measure(scene, window.innerHeight));
      observer.observe(scene.element);
    }

    // The pointer's position over `[data-spotlight]` elements, for highlights that follow it.
    const onPointer = (event: PointerEvent) => {
      const target = (event.target as Element | null)?.closest<HTMLElement>("[data-spotlight]");
      if (!target) return;
      const rect = target.getBoundingClientRect();
      target.dataset.pointer = "";
      target.style.setProperty(
        "--mx",
        `${(((event.clientX - rect.left) / rect.width) * 100).toFixed(1)}%`,
      );
      target.style.setProperty(
        "--my",
        `${(((event.clientY - rect.top) / rect.height) * 100).toFixed(1)}%`,
      );
      // Also as fractions (0–1), for tilting towards the pointer.
      target.style.setProperty("--px", ((event.clientX - rect.left) / rect.width).toFixed(3));
      target.style.setProperty("--py", ((event.clientY - rect.top) / rect.height).toFixed(3));
    };

    // Reduce Motion turned on while reading: everything to rest.
    const reduce = matchMedia("(prefers-reduced-motion: reduce)");
    const onReduce = () => {
      if (!reduce.matches) return;
      root.classList.remove("tt-motion");
      for (const scene of scenes) apply(scene, scene.kind === "stack" ? 0 : 1);
    };

    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", onResize, { passive: true });
    document.addEventListener("pointermove", onPointer, { passive: true });
    reduce.addEventListener("change", onReduce);
    return () => {
      observer.disconnect();
      heights.disconnect();
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("pointermove", onPointer);
      reduce.removeEventListener("change", onReduce);
    };
  }, [pathname]);
  return null;
}
