import { Accordion, Accordions } from "fumadocs-ui/components/accordion";
import { Callout } from "fumadocs-ui/components/callout";
import { Card, Cards } from "fumadocs-ui/components/card";
import { Step, Steps } from "fumadocs-ui/components/steps";
import { Tab, Tabs } from "fumadocs-ui/components/tabs";
import defaultMdxComponents from "fumadocs-ui/mdx";
import type { MDXComponents } from "mdx/types";
import { Downloads } from "./downloads";
import { Shot } from "./landing";

/** A screenshot of the app in the docs, in the reader's color scheme, with a caption. */
function Screenshot({
  name,
  alt,
  caption,
  width,
  height,
  narrow = false,
}: {
  name: string;
  alt: string;
  caption?: string;
  width?: number;
  height?: number;
  /** Small windows (Settings) are shown at their own size rather than full width. */
  narrow?: boolean;
}) {
  return (
    <figure className={`not-prose my-6 ${narrow ? "max-w-md" : ""}`}>
      <Shot
        name={name}
        alt={alt}
        lights={narrow ? "settings" : "main"}
        {...(width && height ? { size: { width, height } } : {})}
      />
      {caption ? (
        <figcaption className="mt-2 text-center text-sm text-fd-muted-foreground">
          {caption}
        </figcaption>
      ) : null}
    </figure>
  );
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    Accordion,
    Accordions,
    Callout,
    Card,
    Cards,
    Downloads,
    Screenshot,
    Step,
    Steps,
    Tab,
    Tabs,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
