// Bundles the extension (and the shared control client) into dist/extension.js.
// `--production` minifies, `--watch` rebuilds on change.
import { build, context } from "esbuild";

const production = process.argv.includes("--production");
const options = {
  entryPoints: ["src/extension.ts"],
  bundle: true,
  outfile: "dist/extension.js",
  platform: "node",
  format: "cjs",
  target: "node20",
  external: ["vscode"],
  sourcemap: !production,
  minify: production,
  logLevel: "info",
};

if (process.argv.includes("--watch")) {
  const ctx = await context(options);
  await ctx.watch();
} else {
  await build(options);
}
