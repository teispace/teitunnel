"use client";

import { useEffect, useRef } from "react";

/** How many arches are in the tunnel at once. */
const ARCHES = 16;
/** Depth at which an arch appears (far) and the nearest it gets before starting again. */
const FAR = 1;
const NEAR = 0.06;
/** How fast the tunnel comes towards the reader, in depth per second. */
const SPEED = 0.028;

interface Arch {
  z: number;
}

/** A request riding an arch, from one foot over the top to the other. */
interface Rider {
  arch: number;
  u: number;
  speed: number;
}

/** A request sent by a click: from the pointer into the tunnel. */
interface Shot {
  x: number;
  y: number;
  t: number;
}

interface Colors {
  line: string;
  accent: string;
}

/** The theme's colours, as the canvas needs them. */
function readColors(element: HTMLElement): Colors {
  const style = getComputedStyle(element);
  return {
    line: style.getPropertyValue("--color-fd-foreground").trim() || "#888",
    accent: style.getPropertyValue("--tt-accent").trim() || "#3b82f6",
  };
}

/**
 * The hero's background: Teitunnel's arch as a tunnel receding to a vanishing point, arches
 * drifting slowly towards the reader with requests riding them. The tunnel turns towards the
 * pointer, a soft light follows it, and a click sends a request into the tunnel. Drawn only
 * while on screen; one still frame with Reduce Motion.
 */
export function HeroTunnel() {
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
    const arches: Arch[] = Array.from({ length: ARCHES }, (_, i) => ({
      z: NEAR + ((FAR - NEAR) * (i + 0.5)) / ARCHES,
    }));
    const riders: Rider[] = Array.from({ length: 5 }, (_, i) => ({
      arch: (i * 3) % ARCHES,
      u: Math.random(),
      speed: 0.05 + Math.random() * 0.06,
    }));
    const shots: Shot[] = [];
    // Where the pointer is (0–1 across the hero), eased.
    const pointer = { x: 0.5, y: 0.4, tx: 0.5, ty: 0.4, inside: false };

    const resize = () => {
      const rect = host.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };

    /** An arch's center and half-width at depth `z`. */
    const place = (z: number) => {
      const scale = NEAR / z;
      const base = Math.min(width, 1200) * 0.42;
      const vanishX = width * (0.5 + (pointer.x - 0.5) * 0.16);
      const vanishY = height * (0.34 + (pointer.y - 0.4) * 0.12);
      // Far arches sit at the vanishing point; near ones around the middle of the view.
      const near = Math.min(1, scale * 1.4);
      return {
        x: vanishX + (width / 2 - vanishX) * near,
        y: vanishY + (height * 0.42 - vanishY) * near,
        r: base * scale * 7,
      };
    };

    /** A point on an arch: `u` 0 at the left foot, 0.5 at the top, 1 at the right foot. */
    const along = (z: number, u: number) => {
      const { x, y, r } = place(z);
      const leg = r * 1.1;
      const total = leg * 2 + Math.PI * r;
      let d = u * total;
      if (d < leg) return { x: x - r, y: y + leg - d };
      d -= leg;
      if (d < Math.PI * r) {
        const angle = Math.PI + d / r;
        return { x: x + Math.cos(angle) * r, y: y + Math.sin(angle) * r };
      }
      d -= Math.PI * r;
      return { x: x + r, y: y + d };
    };

    const alpha = (z: number) =>
      Math.max(0, Math.min(1, (FAR - z) * 5)) * Math.max(0, Math.min(1, (z - NEAR) * 9));

    const draw = () => {
      context.clearRect(0, 0, width, height);
      // The light that follows the pointer.
      if (pointer.inside) {
        const glow = context.createRadialGradient(
          pointer.x * width,
          pointer.y * height,
          0,
          pointer.x * width,
          pointer.y * height,
          260,
        );
        glow.addColorStop(0, colors.accent);
        glow.addColorStop(1, "transparent");
        context.globalAlpha = 0.12;
        context.fillStyle = glow;
        context.fillRect(0, 0, width, height);
        context.globalAlpha = 1;
      }
      context.strokeStyle = colors.line;
      context.lineWidth = 1;
      for (const arch of arches) {
        const { x, y, r } = place(arch.z);
        const leg = r * 1.1;
        context.globalAlpha = alpha(arch.z) * 0.16;
        context.beginPath();
        context.moveTo(x - r, y + leg);
        context.lineTo(x - r, y);
        context.arc(x, y, r, Math.PI, 0);
        context.lineTo(x + r, y + leg);
        context.stroke();
      }
      context.globalAlpha = 1;
      context.fillStyle = colors.accent;
      for (const rider of riders) {
        const arch = arches[rider.arch];
        if (!arch) continue;
        const a = alpha(arch.z);
        if (a <= 0.05) continue;
        const point = along(arch.z, rider.u);
        const size = Math.max(1.2, 2.6 * (NEAR / arch.z) * 7);
        context.globalAlpha = a * 0.9;
        context.beginPath();
        context.arc(point.x, point.y, Math.min(size, 3.2), 0, Math.PI * 2);
        context.fill();
      }
      for (const shot of shots) {
        const eased = 1 - (1 - shot.t) ** 3;
        const target = place(FAR * 0.9);
        const x = shot.x + (target.x - shot.x) * eased;
        const y = shot.y + (target.y - shot.y) * eased;
        context.globalAlpha = 1 - shot.t;
        context.beginPath();
        context.arc(x, y, 3.5 * (1 - shot.t * 0.7), 0, Math.PI * 2);
        context.fill();
      }
      context.globalAlpha = 1;
    };

    let frame = 0;
    let last = 0;
    let running = false;
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000 || 0);
      last = now;
      pointer.x += (pointer.tx - pointer.x) * Math.min(1, dt * 3);
      pointer.y += (pointer.ty - pointer.y) * Math.min(1, dt * 3);
      for (const arch of arches) {
        arch.z -= SPEED * dt * (0.4 + arch.z);
        if (arch.z < NEAR) arch.z += FAR - NEAR;
      }
      for (const rider of riders) {
        rider.u += rider.speed * dt;
        if (rider.u > 1) {
          rider.u = 0;
          rider.arch = Math.floor(Math.random() * ARCHES);
        }
      }
      for (let i = shots.length - 1; i >= 0; i--) {
        const shot = shots[i];
        if (!shot) continue;
        shot.t += dt * 1.1;
        if (shot.t >= 1) shots.splice(i, 1);
      }
      draw();
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
      if (still) draw();
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
      shots.push({ x: event.clientX - rect.left, y: event.clientY - rect.top, t: 0 });
      if (shots.length > 12) shots.shift();
    };

    resize();
    draw();
    const sizes = new ResizeObserver(() => {
      resize();
      draw();
    });
    sizes.observe(host);
    const themes = new MutationObserver(() => {
      colors = readColors(canvas);
      draw();
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
