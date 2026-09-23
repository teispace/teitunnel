import { type FormEvent, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useStartShare } from "../queries";
import { ServicePicker } from "./service-picker";

const AUTO_STOPS = ["never", "15", "60", "480"] as const;
type AutoStop = (typeof AUTO_STOPS)[number];
const autoStops = () =>
  AUTO_STOPS.map((value) => ({ value, label: t(`quickShare.autoStop.${value}`) }));

/** Origin field + auto-stop + one primary action. */
export function ShareComposer({
  disabled = false,
  autoFocus = false,
}: {
  disabled?: boolean;
  autoFocus?: boolean;
}) {
  const [origin, setOrigin] = useState("");
  const [autoStop, setAutoStop] = useState<AutoStop>("never");
  const start = useStartShare();
  const errorId = useId();
  const error = start.error ? toIpcError(start.error) : null;
  const fieldError = error?.field === "origin" ? error.message : null;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (disabled || start.isPending) return;
    start.mutate(
      { origin, stopAfterMinutes: autoStop === "never" ? null : Number(autoStop) },
      { onSuccess: () => setOrigin("") },
    );
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-1.5" noValidate>
      <div className="flex items-center gap-2">
        <ServicePicker
          value={origin}
          onChange={(value) => {
            setOrigin(value);
            if (start.error) start.reset();
          }}
          autoFocus={autoFocus}
          invalid={fieldError !== null}
          describedBy={fieldError ? errorId : undefined}
        />
        <Select
          label={t("quickShare.whenToStop")}
          options={autoStops()}
          value={autoStop}
          onValueChange={setAutoStop}
          className="h-7"
        />
        <Button type="submit" variant="primary" size="lg" disabled={disabled || start.isPending}>
          {t("quickShare.share")}
        </Button>
      </div>
      {error && error.code !== "cloudflaredMissing" ? (
        <p id={errorId} role="alert" className="px-1 text-callout text-error">
          {error.message}
          {error.hint ? <span className="text-secondary"> {error.hint}</span> : null}
        </p>
      ) : null}
    </form>
  );
}
