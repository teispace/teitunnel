"use client";

import { useEffect, useRef } from "react";

/** Stars per 100,000 square pixels, and how many depths they come in. */
const DENSITY = 9;
const LAYERS = 3;
/** Seconds between the comets that cross on their own. */
const EVERY = 2.6;
/** Points kept for a comet's trail. */
const TRAIL = 26;

interface Star {
  x: number;
  y: number;
  /** 0 far … 1 near. */
  depth: number;
  size: number;
  phase: number;
}

/** A request crossing the sky: from where it starts, along a gentle curve, to where it goes. */
interface Comet {
  from: { x: number; y: number };
  bend: { x: number; y: number };
  to: { x: number; y: number };
  t: number;
  speed: number;
  trail: { x: number; y: number }[];
}

interface Colors {
  star: string;
  accent: string;
}

function readColors(element: HTMLElement): Colors {
  const style = getComputedStyle(element);
  return {
    star: style.getPropertyValue("--color-fd-foreground").trim() || "#999",
    accent: style.getPropertyValue("--tt-accent").trim() || "#3b82f6",
  };
}

/** A point on a quadratic curve. */
function curve(c: Comet, t: number) {
  const a = (1 - t) * (1 - t);
  const b = 2 * (1 - t) * t;
  const d = t * t;
  return {
    x: a * c.from.x + b * c.bend.x + d * c.to.x,
    y: a * c.from.y + b * c.bend.y + d * c.to.y,
  };
}

/**
 * The hero's background: a quiet field of stars in three depths that drifts and leans with
 * the pointer, a soft light that follows it, and now and then a request crossing the sky with
 * a fading trail; a click sends one from the pointer. Drawn only while on screen; one still
 * frame with Reduce Motion.
 */
export function HeroSpace() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    const context = canvas?.getContext("2d");
    if (!canvas || !host || !context) return;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

    let width = 0;
    let height = 0;
    let colors = readColors(canvas);
    let stars: Star[] = [];
    const comets: Comet[] = [];
    const pointer = { x: 0.5, y: 0.4, tx: 0.5, ty: 0.4, inside: false };
    let drift = 0;
    let untilNext = 1.2;

    const seed = () => {
      const count = Math.round(((width * height) / 100_000) * DENSITY);
      stars = Array.from({ length: count }, () => {
        const depth = Math.floor(Math.random() * LAYERS) / (LAYERS - 1);
        return {
          x: Math.random(),
          y: Math.random(),
          depth,
          size: 0.6 + depth * 0.9 + Math.random() * 0.3,
          phase: Math.random() * Math.PI * 2,
        };
      });
    };

    const resize = () => {
      const rect = host.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      seed();
    };

    /** A comet from `from` towards the app window below the headline. */
    const launch = (from?: { x: number; y: number }) => {
      const side = Math.random() < 0.5 ? -1 : 1;
      const start = from ?? {
        x: side < 0 ? -20 : width + 20,
        y: height * (0.05 + Math.random() * 0.35),
      };
      const to = {
        x: width * (0.5 + (Math.random() - 0.5) * 0.3),
        y: height * (0.62 + Math.random() * 0.08),
      };
      const bend = {
        x: (start.x + to.x) / 2 + (Math.random() - 0.5) * width * 0.25,
        y: Math.min(start.y, to.y) - height * (0.05 + Math.random() * 0.15),
      };
      comets.push({ from: start, bend, to, t: 0, speed: 0.22 + Math.random() * 0.1, trail: [] });
      if (comets.length > 8) comets.shift();
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, width, height);
      const px = pointer.x * width;
      const py = pointer.y * height;
      if (pointer.inside) {
        const glow = context.createRadialGradient(px, py, 0, px, py, 280);
        glow.addColorStop(0, colors.accent);
        glow.addColorStop(1, "transparent");
        context.globalAlpha = 0.14;
        context.fillStyle = glow;
        context.fillRect(0, 0, width, height);
      }

      // Stars: the near ones drift faster and lean further with the pointer.
      context.fillStyle = colors.star;
      const lean = { x: (pointer.x - 0.5) * 28, y: (pointer.y - 0.4) * 18 };
      for (const star of stars) {
        const speed = 4 + star.depth * 10;
        let x = (star.x * width - drift * speed - lean.x * (0.3 + star.depth)) % width;
        if (x < 0) x += width;
        const y = star.y * height - lean.y * (0.3 + star.depth);
        const twinkle = 0.65 + 0.35 * Math.sin(time / 1400 + star.phase);
        const near = pointer.inside ? Math.max(0, 1 - Math.hypot(x - px, y - py) / 180) : 0;
        context.globalAlpha = (0.12 + star.depth * 0.22) * twinkle + near * 0.45;
        context.beginPath();
        context.arc(x, y, star.size + near * 0.8, 0, Math.PI * 2);
        context.fill();
      }

      // Requests crossing: a bright head and a trail that thins and fades.
      context.lineCap = "round";
      for (const comet of comets) {
        const fade = comet.t > 0.8 ? (1 - comet.t) / 0.2 : 1;
        for (let i = 1; i < comet.trail.length; i++) {
          const a = comet.trail[i - 1];
          const b = comet.trail[i];
          if (!a || !b) continue;
          const k = i / comet.trail.length;
          context.globalAlpha = k * 0.55 * fade;
          context.strokeStyle = colors.accent;
          context.lineWidth = 0.4 + k * 1.8;
          context.beginPath();
          context.moveTo(a.x, a.y);
          context.lineTo(b.x, b.y);
          context.stroke();
        }
        const head = comet.trail[comet.trail.length - 1];
        if (head) {
          context.globalAlpha = 0.9 * fade;
          context.fillStyle = colors.accent;
          context.beginPath();
          context.arc(head.x, head.y, 2.2, 0, Math.PI * 2);
          context.fill();
          context.globalAlpha = 0.25 * fade;
          context.beginPath();
          context.arc(head.x, head.y, 6, 0, Math.PI * 2);
          context.fill();
        }
      }
      context.globalAlpha = 1;
    };

    let frame = 0;
    let last = 0;
    let running = false;
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000 || 0);
      last = now;
      drift += dt;
      pointer.x += (pointer.tx - pointer.x) * Math.min(1, dt * 2.5);
      pointer.y += (pointer.ty - pointer.y) * Math.min(1, dt * 2.5);
      untilNext -= dt;
      if (untilNext <= 0) {
        launch();
        untilNext = EVERY * (0.7 + Math.random() * 0.6);
      }
      for (let i = comets.length - 1; i >= 0; i--) {
        const comet = comets[i];
        if (!comet) continue;
        comet.t = Math.min(1, comet.t + comet.speed * dt);
        comet.trail.push(curve(comet, comet.t));
        if (comet.trail.length > TRAIL) comet.trail.shift();
        if (comet.t >= 1 && comet.trail.length > 0) comet.trail.shift();
        if (comet.t >= 1 && comet.trail.length === 0) comets.splice(i, 1);
      }
      draw(now);
      frame = running ? requestAnimationFrame(tick) : 0;
    };
    const start = () => {
      if (running || still) return;
      running = true;
      last = performance.now();
      frame = requestAnimationFrame(tick);
    };
    const stop = () => {
      running = false;
      cancelAnimationFrame(frame);
      frame = 0;
    };

    const onMove = (event: PointerEvent) => {
      const rect = host.getBoundingClientRect();
      pointer.tx = (event.clientX - rect.left) / rect.width;
      pointer.ty = (event.clientY - rect.top) / rect.height;
      pointer.inside = pointer.ty >= 0 && pointer.ty <= 1;
    };
    const onLeave = () => {
      pointer.inside = false;
      pointer.tx = 0.5;
      pointer.ty = 0.4;
    };
    const onDown = (event: PointerEvent) => {
      // Clicks on links and buttons are theirs.
      if ((event.target as Element | null)?.closest("a, button, input, [role=button]")) return;
      const rect = host.getBoundingClientRect();
      launch({ x: event.clientX - rect.left, y: event.clientY - rect.top });
    };

    resize();
    draw(0);
    const sizes = new ResizeObserver(() => {
      resize();
      draw(performance.now());
    });
    sizes.observe(host);
    const themes = new MutationObserver(() => {
      colors = readColors(canvas);
      draw(performance.now());
    });
    themes.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    const view = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting && !document.hidden) start();
      else stop();
    });
    view.observe(host);
    const onVisibility = () => (document.hidden ? stop() : start());
    document.addEventListener("visibilitychange", onVisibility);
    host.addEventListener("pointermove", onMove, { passive: true });
    host.addEventListener("pointerleave", onLeave);
    host.addEventListener("pointerdown", onDown);
    return () => {
      stop();
      sizes.disconnect();
      themes.disconnect();
      view.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
      host.removeEventListener("pointermove", onMove);
      host.removeEventListener("pointerleave", onLeave);
      host.removeEventListener("pointerdown", onDown);
    };
  }, []);

  return <canvas ref={canvasRef} aria-hidden className="tt-hero-canvas" />;
}
