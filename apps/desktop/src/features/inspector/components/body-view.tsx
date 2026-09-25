import { useMemo, useState } from "react";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { formatBytes } from "@/features/snapshots/format";
import { type MessageKey, t } from "@/lib/i18n";
import type { BodyView as Body, ContentKind } from "@/lib/ipc/bindings";
import {
  fromBase64,
  HEX_LIMIT,
  hexDump,
  parseForm,
  parseMultipart,
  prettyJson,
  utf8,
} from "../model";

type Mode = "pretty" | "form" | "parts" | "preview" | "raw" | "hex";

/** Ways to show each kind of body, the most useful first. */
const modesFor: Record<ContentKind, readonly Mode[]> = {
  empty: [],
  json: ["pretty", "raw", "hex"],
  form: ["form", "raw", "hex"],
  multipart: ["parts", "raw"],
  image: ["preview", "hex"],
  binary: ["hex"],
  html: ["raw", "hex"],
  xml: ["raw", "hex"],
  text: ["raw", "hex"],
  eventStream: ["raw", "hex"],
};

/** Characters rendered at most (a 1 MiB capture would make the pane sluggish). */
const TEXT_LIMIT = 200_000;

const pre =
  "selectable max-h-[480px] overflow-auto rounded-control bg-surface-inset px-2 py-1.5 font-mono text-mono whitespace-pre-wrap break-all";

/** A captured body: pretty JSON, form fields, multipart parts, an image, hex or text. */
export function BodyView({ body, label }: { body: Body; label: string }) {
  const modes = modesFor[body.kind];
  const [chosen, setChosen] = useState<Mode | null>(null);
  const mode = chosen && modes.includes(chosen) ? chosen : (modes[0] ?? null);

  if (body.kind === "empty" || (body.size === 0 && !body.text && !body.base64)) {
    return <p className="text-callout text-secondary">{t("inspector.body.none")}</p>;
  }

  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex items-center gap-2 text-callout text-secondary">
        <span className="min-w-0 flex-1 truncate">
          {[body.contentType, formatBytes(body.size), body.encoding].filter(Boolean).join(" · ")}
        </span>
        {modes.length > 1 && mode ? (
          <SegmentedControl
            label={t("inspector.body.viewAs", { label })}
            size="sm"
            segments={modes.map((value) => ({
              value,
              label: t(`inspector.body.mode.${value}` as MessageKey),
            }))}
            value={mode}
            onValueChange={setChosen}
          />
        ) : null}
      </div>
      {mode ? <Content body={body} mode={mode} /> : null}
      {body.truncated ? (
        <p className="text-callout text-secondary">
          {t("inspector.body.truncated", {
            captured: formatBytes(body.captured),
            size: formatBytes(body.size),
          })}
        </p>
      ) : null}
      {body.decodeError ? (
        <p className="text-callout text-warning">
          {t("inspector.body.decodeError", { detail: body.decodeError })}
        </p>
      ) : null}
    </div>
  );
}

function Content({ body, mode }: { body: Body; mode: Mode }) {
  const text = body.text ?? "";
  switch (mode) {
    case "pretty":
      return <Text text={prettyJson(text) ?? text} />;
    case "raw":
      return <Text text={text} />;
    case "form": {
      const fields = parseForm(text);
      return fields.length === 0 ? (
        <Text text={text} />
      ) : (
        <KeyValueGrid
          className="rounded-control bg-surface-inset px-2 py-1.5"
          items={fields.map(([name, value], index) => ({
            // Repeated names are kept apart.
            label: fields.findIndex(([n]) => n === name) === index ? name : `${name} (${index})`,
            value,
            mono: true,
          }))}
        />
      );
    }
    case "parts":
      return <Parts text={text} contentType={body.contentType} />;
    case "preview":
      return body.base64 ? (
        <img
          alt={t("inspector.body.image")}
          src={`data:${body.contentType ?? "image/png"};base64,${body.base64}`}
          className="max-h-80 max-w-full self-start rounded-control bg-surface-inset object-contain"
        />
      ) : (
        <Text text={text} />
      );
    case "hex":
      return <Hex body={body} />;
  }
}

function Text({ text }: { text: string }) {
  const shown = text.length > TEXT_LIMIT ? text.slice(0, TEXT_LIMIT) : text;
  return (
    <>
      <pre className={pre}>{shown}</pre>
      {shown.length < text.length ? (
        <p className="text-callout text-secondary">
          {t("inspector.body.shortened", { count: TEXT_LIMIT })}
        </p>
      ) : null}
    </>
  );
}

function Hex({ body }: { body: Body }) {
  const bytes = useMemo(
    () => (body.base64 ? fromBase64(body.base64) : utf8(body.text ?? "")),
    [body.base64, body.text],
  );
  return (
    <>
      <pre className={`${pre} whitespace-pre break-normal`}>{hexDump(bytes)}</pre>
      {bytes.length > HEX_LIMIT ? (
        <p className="text-callout text-secondary">
          {t("inspector.body.hexShortened", { size: formatBytes(HEX_LIMIT) })}
        </p>
      ) : null}
    </>
  );
}

function Parts({ text, contentType }: { text: string; contentType: string | null }) {
  const parts = parseMultipart(text, contentType);
  if (parts.length === 0) return <Text text={text} />;
  return (
    <ol className="flex flex-col gap-2">
      {parts.map((part, index) => (
        <li
          // biome-ignore lint/suspicious/noArrayIndexKey: parts are positional
          key={index}
          className="flex flex-col gap-1 rounded-control bg-surface-inset px-2 py-1.5"
        >
          <span className="text-callout font-medium">
            {part.filename
              ? t("inspector.body.file", { name: part.name ?? "", file: part.filename })
              : (part.name ?? t("inspector.body.part", { number: index + 1 }))}
          </span>
          <KeyValueGrid
            items={part.headers.map(([name, value]) => ({ label: name, value, mono: true }))}
          />
          {part.body ? (
            <pre className="selectable max-h-40 overflow-auto font-mono text-mono whitespace-pre-wrap break-all">
              {part.body.length > 4000 ? `${part.body.slice(0, 4000)}…` : part.body}
            </pre>
          ) : null}
        </li>
      ))}
    </ol>
  );
}
