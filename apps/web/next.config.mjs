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
};

export default createMDX()(config);
