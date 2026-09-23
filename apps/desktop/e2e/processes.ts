import { execFileSync } from "node:child_process";

/**
 * Command lines of every running process, normalized so tests can match them the same
 * way on every platform: Windows paths lose their quotes and the `.exe` suffix.
 */
export function commandLines(): string[] {
  if (process.platform === "win32") {
    const out = execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process | ForEach-Object { $_.CommandLine }",
      ],
      { encoding: "utf8" },
    );
    return out
      .split(/\r?\n/)
      .map((line) =>
        line
          .replaceAll('"', "")
          .replace(/\.exe\b/i, "")
          .trim(),
      )
      .filter(Boolean);
  }
  return execFileSync("ps", ["-axo", "command="], { encoding: "utf8" })
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}
