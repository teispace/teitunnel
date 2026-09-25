import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { type MessageKey, t } from "@/lib/i18n";
import type { EdgeHeaderOp, HeaderRule } from "@/lib/ipc/bindings";

const opLabels: Record<EdgeHeaderOp, MessageKey> = {
  set: "protection.headers.op.set",
  add: "protection.headers.op.add",
  remove: "protection.headers.op.remove",
};

interface HeaderRulesProps {
  /** `request` headers can be set or removed; `response` ones also added. */
  kind: "request" | "response";
  value: readonly HeaderRule[];
  onChange: (rules: HeaderRule[]) => void;
}

/** Header rules as rows: name, what to do, value; add and remove rows. */
export function HeaderRules({ kind, value, onChange }: HeaderRulesProps) {
  const ops: EdgeHeaderOp[] = kind === "request" ? ["set", "remove"] : ["set", "add", "remove"];
  const label =
    kind === "request" ? t("protection.headers.request") : t("protection.headers.response");
  const update = (index: number, rule: HeaderRule) =>
    onChange(value.map((current, i) => (i === index ? rule : current)));

  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-1 text-body">{label}</legend>
      {value.length === 0 ? (
        <p className="text-callout text-secondary">{t("protection.headers.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {value.map((rule, index) => (
            // Rows are positional while editing; the index is their identity.
            // biome-ignore lint/suspicious/noArrayIndexKey: positional list
            <li key={index} className="flex items-center gap-1.5">
              <Input
                aria-label={t("protection.headers.name", { index: index + 1 })}
                placeholder={kind === "request" ? "X-Env" : "X-Robots-Tag"}
                autoComplete="off"
                spellCheck={false}
                className="w-36 font-mono text-mono"
                value={rule.name}
                onChange={(event) => update(index, { ...rule, name: event.target.value })}
              />
              <Select
                label={t("protection.headers.action", { index: index + 1 })}
                options={ops.map((op) => ({ value: op, label: t(opLabels[op]) }))}
                value={rule.op}
                onValueChange={(op) =>
                  update(index, { ...rule, op, value: op === "remove" ? null : (rule.value ?? "") })
                }
                className="w-24"
              />
              <Input
                aria-label={t("protection.headers.value", { index: index + 1 })}
                placeholder={rule.op === "remove" ? "" : kind === "request" ? "preview" : "noindex"}
                autoComplete="off"
                spellCheck={false}
                disabled={rule.op === "remove"}
                className="min-w-0 flex-1 font-mono text-mono"
                value={rule.value ?? ""}
                onChange={(event) => update(index, { ...rule, value: event.target.value })}
              />
              <IconButton
                icon={X}
                label={t("protection.headers.removeRow", { index: index + 1 })}
                size="sm"
                onClick={() => onChange(value.filter((_, i) => i !== index))}
              />
            </li>
          ))}
        </ul>
      )}
      <div>
        <Button
          size="sm"
          variant="plain"
          className="-ml-2.5"
          onClick={() => onChange([...value, { name: "", op: "set", value: "" }])}
        >
          <Plus /> {t("protection.headers.addRow")}
        </Button>
      </div>
    </fieldset>
  );
}
