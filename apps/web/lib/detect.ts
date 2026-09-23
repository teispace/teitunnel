import type { Arch, Os } from "./release";

export interface Platform {
  os: Os | "mobile" | null;
  /** The CPU when the browser says (Chromium on Windows and Linux); null otherwise. */
  arch: Exclude<Arch, "universal"> | null;
}

interface UaData {
  platform?: string;
  mobile?: boolean;
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }>;
}

/** The visitor's system from a user-agent string (the fallback everywhere). */
export function fromUserAgent(ua: string): Platform {
  if (/iPhone|iPad|iPod|Android/i.test(ua)) return { os: "mobile", arch: null };
  const arch = /aarch64|arm64/i.test(ua) ? "arm64" : null;
  if (/Macintosh|Mac OS X/i.test(ua)) return { os: "macos", arch: null };
  if (/Windows/i.test(ua)) return { os: "windows", arch };
  if (/Linux|X11|CrOS/i.test(ua))
    return { os: "linux", arch: arch ?? (/x86_64|amd64/i.test(ua) ? "x64" : null) };
  return { os: null, arch: null };
}

/**
 * The visitor's system. User-Agent Client Hints (Chromium) tell Windows on Arm apart;
 * Safari can't tell Apple silicon from Intel, which is why the Mac download is universal.
 */
export async function detectPlatform(): Promise<Platform> {
  const base = fromUserAgent(navigator.userAgent);
  const data = (navigator as Navigator & { userAgentData?: UaData }).userAgentData;
  if (!data?.getHighEntropyValues || base.os === "mobile" || base.os === "macos") return base;
  try {
    const { architecture } = await data.getHighEntropyValues(["architecture"]);
    if (architecture === "arm") return { ...base, arch: "arm64" };
    if (architecture === "x86") return { ...base, arch: "x64" };
  } catch {
    // Hints refused: keep the user-agent guess.
  }
  return base;
}
