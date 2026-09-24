/**
 * Guessing the workspace's dev server port from its `package.json` scripts (and a few
 * other frameworks' files), without running anything. Pure: no `vscode` import.
 */

/** A port the workspace's dev server probably listens on, and why we think so. */
export interface PortGuess {
  port: number;
  /** E.g. `dev (vite)` or `manage.py`. */
  source: string;
}

/** Scripts that start a dev server, most likely first. */
const DEV_SCRIPTS = ["dev", "start", "serve", "develop", "preview", "storybook"];

/** A tool in a script, and the port it uses when none is given. */
const DEFAULT_PORTS: [RegExp, number, string][] = [
  [/\bnext\s+(dev|start)\b/, 3000, "next"],
  [/\bnuxi?\s+dev\b/, 3000, "nuxt"],
  [/\bastro\s+(dev|preview)\b/, 4321, "astro"],
  [/\b(ng|ng\.js)\s+serve\b/, 4200, "angular"],
  [/\bstorybook\s+dev\b|\bstart-storybook\b/, 6006, "storybook"],
  [/\bgatsby\s+develop\b/, 8000, "gatsby"],
  [/\breact-scripts\s+start\b/, 3000, "create-react-app"],
  [/\bvue-cli-service\s+serve\b/, 8080, "vue-cli"],
  [/\bwebpack(-dev-server|\s+serve)\b/, 8080, "webpack"],
  [/\bparcel\b/, 1234, "parcel"],
  [/\b(eleventy|@11ty)\b.*--serve/, 8080, "eleventy"],
  [/\bremix\s+dev\b/, 3000, "remix"],
  [/\bvite\s+preview\b/, 4173, "vite preview"],
  [/\bvite\b|\bsvelte-kit\s+dev\b|\breact-router\s+dev\b/, 5173, "vite"],
  [/\bwrangler\s+(dev|pages\s+dev)\b/, 8787, "wrangler"],
  [/\bhugo\s+server\b/, 1313, "hugo"],
];

/** A port given in the script itself: `--port 3001`, `-p 3001`, `--port=3001`, `PORT=3001`. */
export function explicitPort(script: string): number | undefined {
  const match =
    /(?:--port[= ]|(?:^|\s)-p\s+|\bPORT=)(\d{2,5})\b/.exec(script) ??
    /\blocalhost:(\d{2,5})\b/.exec(script);
  const port = match?.[1] ? Number(match[1]) : undefined;
  return port && port > 0 && port < 65536 ? port : undefined;
}

/** Guesses from a `package.json`'s text, dev scripts first. */
export function guessFromPackageJson(text: string): PortGuess[] {
  let scripts: Record<string, unknown>;
  try {
    const parsed = JSON.parse(text) as { scripts?: unknown };
    scripts =
      parsed.scripts && typeof parsed.scripts === "object"
        ? (parsed.scripts as Record<string, unknown>)
        : {};
  } catch {
    return [];
  }
  const names = [
    ...DEV_SCRIPTS.filter((name) => name in scripts),
    ...Object.keys(scripts).filter(
      (name) => !DEV_SCRIPTS.includes(name) && /dev|serve|start/.test(name),
    ),
  ];
  const guesses: PortGuess[] = [];
  for (const name of names) {
    const script = scripts[name];
    if (typeof script !== "string") continue;
    const port = explicitPort(script);
    const tool = DEFAULT_PORTS.find(([pattern]) => pattern.test(script));
    if (port) guesses.push({ port, source: tool ? `${name} (${tool[2]})` : name });
    else if (tool) guesses.push({ port: tool[1], source: `${name} (${tool[2]})` });
  }
  return unique(guesses);
}

/** Guesses from other frameworks' marker files present in the folder. */
export function guessFromFiles(files: readonly string[]): PortGuess[] {
  const has = (name: string) => files.includes(name);
  const guesses: PortGuess[] = [];
  if (has("manage.py")) guesses.push({ port: 8000, source: "manage.py (django)" });
  if (has("bin/rails") || has("config.ru")) guesses.push({ port: 3000, source: "rails" });
  if (has("artisan")) guesses.push({ port: 8000, source: "artisan (laravel)" });
  if (has("_config.yml") && has("Gemfile")) guesses.push({ port: 4000, source: "jekyll" });
  if (has("hugo.toml") || has("hugo.yaml")) guesses.push({ port: 1313, source: "hugo" });
  if (has("app.py") || has("wsgi.py")) guesses.push({ port: 5000, source: "flask" });
  return guesses;
}

/** Keeps the first guess for each port. */
export function unique(guesses: readonly PortGuess[]): PortGuess[] {
  const seen = new Set<number>();
  return guesses.filter((guess) => !seen.has(guess.port) && seen.add(guess.port));
}

/**
 * The port of an item in VS Code's Ports view, as its context menu passes it
 * (`remoteHost`, `remotePort`, `localAddress`, …): in a remote window the forwarded
 * local port, else the port itself.
 */
export function portFromPortsItem(item: unknown): number | undefined {
  if (!item || typeof item !== "object") return undefined;
  const { localAddress, remotePort } = item as { localAddress?: unknown; remotePort?: unknown };
  if (typeof localAddress === "string") {
    const match = /:(\d{1,5})\/?$/.exec(localAddress);
    if (match?.[1]) return Number(match[1]);
  }
  return typeof remotePort === "number" && remotePort > 0 ? remotePort : undefined;
}

/** A port typed by the person: `3000`, `:3000`, `localhost:3000` or a local URL. */
export function parsePortInput(input: string): string | undefined {
  const text = input.trim();
  if (/^\d{1,5}$/.test(text)) return Number(text) > 0 && Number(text) < 65536 ? text : undefined;
  if (/^:\d{1,5}$/.test(text)) return text.slice(1);
  if (/^(https?:\/\/)?[\w.-]+(:\d{1,5})?\/?$/.test(text) && text.includes(":")) return text;
  return undefined;
}
