import { createFromSource } from "fumadocs-core/search/server";
import { source } from "@/lib/source";

// Built once into a static index (the site is a static export).
export const revalidate = false;
export const { staticGET: GET } = createFromSource(source, { language: "english" });
