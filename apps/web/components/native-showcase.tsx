"use client";

import { Bell, Command, Gauge, Monitor, Moon, Sun } from "lucide-react";
import { type CSSProperties, useEffect, useRef, useState } from "react";
import { asset } from "@/lib/site";
import { screens } from "./landing";

type Platform = "macos" | "windows" | "linux";
type Scheme = "light" | "dark";

const platforms: { id: Platform; label: string; shot: string }[] = [
  { id: "macos", label: "macOS", shot: "overview" },
  { id: "windows", label: "Windows", shot: "overview-windows" },
  { id: "linux", label: "Linux", shot: "overview-linux" },
];

/** How long each system is shown before the next, while nobody has chosen one. */
const DWELL = 4500;

/** The modifier key of each system, for the shortcuts shown. */
const modifier: Record<Platform, string> = { macos: "⌘", windows: "Ctrl", linux: "Ctrl" };

/** The window's frame: what each system draws around the app. */
function Frame({ platform }: { platform: Platform }) {
  if (platform === "windows") {
    return (
      <div className="tt-os-bar tt-os-windows" aria-hidden>
        <span className="flex items-center gap-2">
          <span className="tt-os-icon" />
          Teitunnel
        </span>
        <span className="flex h-full">
          <span className="tt-os-caption">
            <svg aria-hidden="true" viewBox="0 0 10 10" className="size-2.5">
              <path d="M0 5h10" stroke="currentColor" />
            </svg>
          </span>
          <span className="tt-os-caption">
            <svg aria-hidden="true" viewBox="0 0 10 10" className="size-2.5">
              <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
            </svg>
          </span>
          <span className="tt-os-caption tt-os-close">
            <svg aria-hidden="true" viewBox="0 0 10 10" className="size-2.5">
              <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" />
            </svg>
          </span>
        </span>
      </div>
    );
  }
  if (platform === "linux") {
    return (
      <div className="tt-os-bar tt-os-linux" aria-hidden>
        <span />
        <span className="font-semibold">Teitunnel</span>
        <span className="flex justify-end">
          <span className="tt-os-gnome-close">
            <svg aria-hidden="true" viewBox="0 0 10 10" className="size-2">
              <path d="M1 1l8 8M9 1L1 9" stroke="currentColor" strokeWidth="1.6" />
            </svg>
          </span>
        </span>
      </div>
    );
  }
  return null;
}

/**
 * "Made to feel at home": one app window, shown as each system draws it. It moves from
 * macOS to Windows to Linux by itself (a line shows how long each stays) until someone
 * picks one; light and dark can be flipped too. The shortcuts listed follow the system.
 */
export function NativeShowcase() {
  const ref = useRef<HTMLDivElement>(null);
  const [platform, setPlatform] = useState<Platform>("macos");
  const [scheme, setScheme] = useState<Scheme | null>(null);
  const [auto, setAuto] = useState(false);
  const [visible, setVisible] = useState(false);

  // Cycle only while on screen, with motion welcome, until someone chooses.
  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    setAuto(true);
    const observer = new IntersectionObserver(([entry]) => setVisible(!!entry?.isIntersecting), {
      threshold: 0.4,
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!auto || !visible) return;
    const timer = window.setTimeout(() => {
      const index = platforms.findIndex((p) => p.id === platform);
      setPlatform(platforms[(index + 1) % platforms.length]?.id ?? "macos");
    }, DWELL);
    return () => window.clearTimeout(timer);
  }, [auto, visible, platform]);

  const choose = (next: Platform) => {
    setAuto(false);
    setPlatform(next);
  };
  const mod = modifier[platform];
  const traits = [
    {
      icon: Monitor,
      title: "Looks like its system",
      body: "System font and accent color, light and dark, native menus, windows and notifications.",
    },
    {
      icon: Command,
      title: "Keyboard first",
      body: (
        <>
          <kbd>{mod}</kbd> <kbd>K</kbd> for any command, <kbd>{mod}</kbd> <kbd>1</kbd>–<kbd>9</kbd>{" "}
          to move around, and a global shortcut that shares your dev server.
        </>
      ),
    },
    {
      icon: Bell,
      title: platform === "macos" ? "In the menu bar" : "In the tray",
      body: "Shares, routes and problems one click away, and a note when a route goes down.",
    },
    {
      icon: Gauge,
      title: "Small and quick",
      body: "Opens in about half a second and stays light while it serves your routes.",
    },
  ];

  return (
    <div
      ref={ref}
      className="grid grid-cols-1 items-center gap-10 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-14"
    >
      <div className="flex flex-col gap-8">
        <div className="flex flex-wrap items-center gap-3">
          <fieldset aria-label="System" className="tt-seg">
            {platforms.map(({ id, label }) => (
              <button
                key={id}
                type="button"
                aria-pressed={platform === id}
                data-active={platform === id ? "" : undefined}
                data-running={platform === id && auto && visible ? "" : undefined}
                onClick={() => choose(id)}
                className="tt-seg-item"
                style={{ "--dwell": `${DWELL}ms` } as CSSProperties}
              >
                {label}
                <span className="tt-seg-bar" aria-hidden />
              </button>
            ))}
          </fieldset>
          <fieldset aria-label="Appearance" className="tt-seg">
            {(
              [
                ["light", Sun, "Light"],
                ["dark", Moon, "Dark"],
              ] as const
            ).map(([value, Icon, label]) => (
              <button
                key={value}
                type="button"
                aria-pressed={scheme === value}
                aria-label={label}
                data-active={scheme === value ? "" : undefined}
                onClick={() => setScheme(scheme === value ? null : value)}
                className="tt-seg-item px-2.5"
              >
                <Icon className="size-4" aria-hidden />
              </button>
            ))}
          </fieldset>
        </div>
        <ul className="flex flex-col gap-5">
          {traits.map(({ icon: Icon, title, body }) => (
            <li key={title} className="flex gap-3">
              <Icon className="mt-0.5 size-5 shrink-0 text-[var(--tt-accent-text)]" aria-hidden />
              <span>
                <span className="block font-medium">{title}</span>
                <span className="tt-kbd-text mt-0.5 block text-sm text-fd-muted-foreground">
                  {body}
                </span>
              </span>
            </li>
          ))}
        </ul>
      </div>

      <figure
        className="tt-os-stage"
        data-platform={platform}
        aria-label={`Teitunnel's Overview on ${platforms.find((p) => p.id === platform)?.label}`}
      >
        {platforms.map(({ id, shot }) => (
          <div
            key={id}
            className="tt-os-window tt-window"
            data-shown={platform === id ? "" : undefined}
            data-scheme={scheme ?? undefined}
            aria-hidden={platform !== id}
          >
            <Frame platform={id} />
            <div className="relative">
              {(["light", "dark"] as const).map((variant) => {
                const hidden =
                  scheme === null
                    ? variant === "light"
                      ? "dark:hidden"
                      : "hidden dark:block"
                    : scheme === variant
                      ? ""
                      : "hidden";
                return (
                  // biome-ignore lint/performance/noImgElement: static export, pre-sized screenshots
                  <img
                    key={variant}
                    src={asset(`/screens/${shot}-${variant}.webp`)}
                    alt=""
                    width={screens.main.width}
                    height={screens.main.height}
                    loading="lazy"
                    decoding="async"
                    className={`block h-auto w-full ${hidden}`}
                  />
                );
              })}
              {id === "macos" ? (
                <span
                  aria-hidden
                  className="absolute flex -translate-y-1/2 gap-[15.4%]"
                  style={{
                    left: `${(20 / screens.main.width) * 200}%`,
                    top: `${(20 / screens.main.height) * 200}%`,
                    width: `${(52 / screens.main.width) * 200}%`,
                  }}
                >
                  <span className="aspect-square flex-1 rounded-full bg-[#ff5f57]" />
                  <span className="aspect-square flex-1 rounded-full bg-[#febc2e]" />
                  <span className="aspect-square flex-1 rounded-full bg-[#28c840]" />
                </span>
              ) : null}
            </div>
          </div>
        ))}
      </figure>
    </div>
  );
}
