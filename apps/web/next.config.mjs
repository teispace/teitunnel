import { createMDX } from "fumadocs-mdx/next";

// Static site, served at the root of https://teitunnel.teispace.com (GitHub Pages with a
// custom domain). BASE_PATH is only for hosting it under a sub-path.
const basePath = process.env.BASE_PATH ?? "";

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  output: "export",
  trailingSlash: true,
  basePath,
  images: { unoptimized: true },
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
  // The repository has its own AGENTS.md; Next's dev server shouldn't write another here.
  agentRules: false,
};

export default createMDX()(config);
