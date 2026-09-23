import { createMDX } from "fumadocs-mdx/next";

// Static site: GitHub Pages serves it under /teitunnel (BASE_PATH=/teitunnel in CI);
// locally it's at the root.
const basePath = process.env.BASE_PATH ?? "";

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  output: "export",
  trailingSlash: true,
  basePath,
  images: { unoptimized: true },
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
};

export default createMDX()(config);
