// Generates src/styles/motion.css from src/lib/motion-tokens.ts.
// Run: node scripts/gen-motion-css.ts   (a Vitest test fails if the file is stale)
import { writeFileSync } from "node:fs";
import { renderMotionCss } from "../src/lib/motion-css.ts";

const out = new URL("../src/styles/motion.css", import.meta.url);
writeFileSync(out, renderMotionCss());
process.stdout.write(`wrote ${out.pathname}\n`);
