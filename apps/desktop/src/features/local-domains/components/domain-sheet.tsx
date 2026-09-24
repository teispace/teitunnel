import { useId, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { ServicePicker } from "@/features/quick-share";
import { t } from "@/lib/i18n";
import type { LocalDomainView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { completeName, suffixOf, targetInput } from "../model";
import { useAddLocalDomain, useUpdateLocalDomain } from "../queries";

interface DomainSheetProps {
  open: boolean;
  onClose: () => void;
  /** Change this domain instead of adding one. */
  editing?: LocalDomainView | null;
  /** Called with the name once added or changed. */
  onDone?: (name: string) => void;
}

function Option({
  id,
  checked,
  onChange,
  label,
  detail,
}: {
  id: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  detail: string;
}) {
  return (
    <label htmlFor={id} className="flex items-start gap-2">
      <Checkbox
        id={id}
        className="mt-0.5"
        checked={checked}
        onCheckedChange={(value) => onChange(value === true)}
      />
      <span className="flex flex-col">
        <span className="text-body">{label}</span>
        <span className="text-callout text-secondary">{detail}</span>
      </span>
    </label>
  );
}

/** Add (or change) a local domain: a name, the service it opens, and its options. */
export function DomainSheet({ open, onClose, editing = null, onDone }: DomainSheetProps) {
  const [name, setName] = useState(editing?.name ?? "");
  const [target, setTarget] = useState(editing ? targetInput(editing.target) : "");
  const [wildcard, setWildcard] = useState(editing?.wildcard ?? false);
  const [https, setHttps] = useState(editing?.https ?? true);
  const [inspect, setInspect] = useState(editing?.inspect ?? false);
  const add = useAddLocalDomain();
  const update = useUpdateLocalDomain();
  const mutation = editing ? update : add;
  const error = mutation.error ? toIpcError(mutation.error) : null;
  const ids = useId();
  const full = completeName(name);
  const suffix = suffixOf(full);

  const close = () => {
    add.reset();
    update.reset();
    onClose();
  };
  const submit = () => {
    const input = { name: full, target: target.trim(), wildcard, https, inspect };
    mutation.mutate(input, {
      onSuccess: (view) => {
        toast.success(
          editing
            ? t("localDomains.sheet.changed", { name: view.name })
            : t("localDomains.sheet.added", { url: view.url }),
        );
        onDone?.(view.name);
        close();
      },
    });
  };
  const canSubmit = full !== "" && target.trim() !== "" && !mutation.isPending;

  return (
    <Sheet open={open} onOpenChange={(next) => (next ? undefined : close())}>
      <SheetContent
        title={editing ? t("localDomains.sheet.editTitle") : t("localDomains.sheet.title")}
        description={t("localDomains.sheet.description")}
        footer={
          <>
            <Button variant="plain" onClick={close}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={submit}
              disabled={!canSubmit}
              pending={mutation.isPending}
            >
              {editing ? t("localDomains.sheet.save") : t("localDomains.sheet.add")}
            </Button>
          </>
        }
      >
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (canSubmit) submit();
          }}
        >
          <Field
            label={t("localDomains.sheet.name")}
            help={
              suffix === "test"
                ? t("localDomains.sheet.nameTest")
                : suffix === "local"
                  ? t("localDomains.sheet.nameLocal")
                  : name.trim() !== "" && full !== name.trim().toLowerCase()
                    ? t("localDomains.sheet.nameCompleted", { name: full })
                    : t("localDomains.sheet.nameHelp")
            }
            error={error?.field === "name" ? error.message : null}
          >
            {(control) => (
              <Input
                {...control}
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t("localDomains.sheet.namePlaceholder")}
                autoFocus={!editing}
                disabled={editing !== null}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
              />
            )}
          </Field>
          <Field
            label={t("localDomains.sheet.target")}
            help={t("localDomains.sheet.targetHelp")}
            error={error?.field === "target" ? error.message : null}
          >
            {(control) => (
              <ServicePicker
                value={target}
                onChange={setTarget}
                autoFocus={editing !== null}
                invalid={control["aria-invalid"] === true}
                describedBy={control["aria-describedby"]}
              />
            )}
          </Field>
          <div className="flex flex-col gap-2.5 pt-1">
            <Option
              id={`${ids}-https`}
              checked={https}
              onChange={setHttps}
              label={t("localDomains.sheet.https")}
              detail={t("localDomains.sheet.httpsDetail")}
            />
            <Option
              id={`${ids}-wildcard`}
              checked={wildcard}
              onChange={setWildcard}
              label={t("localDomains.sheet.wildcard")}
              detail={t("localDomains.sheet.wildcardDetail", { name: full || "shop.localhost" })}
            />
            <Option
              id={`${ids}-inspect`}
              checked={inspect}
              onChange={setInspect}
              label={t("localDomains.sheet.inspect")}
              detail={t("localDomains.sheet.inspectDetail")}
            />
          </div>
          {error && error.field !== "name" && error.field !== "target" ? (
            <p role="alert" className="text-callout text-error">
              {error.message}
            </p>
          ) : null}
        </form>
      </SheetContent>
    </Sheet>
  );
}
