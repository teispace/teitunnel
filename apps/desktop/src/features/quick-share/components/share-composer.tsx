import { type FormEvent, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { toIpcError } from "@/lib/ipc/client";
import { useStartShare } from "../queries";
import { ServicePicker } from "./service-picker";

const autoStops = [
  { value: "never", label: "Keep running" },
  { value: "15", label: "Stop after 15 min" },
  { value: "60", label: "Stop after 1 hour" },
  { value: "480", label: "Stop after 8 hours" },
] as const;

type AutoStop = (typeof autoStops)[number]["value"];

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
          label="When to stop"
          options={autoStops}
          value={autoStop}
          onValueChange={setAutoStop}
          className="h-7"
        />
        <Button type="submit" variant="primary" size="lg" disabled={disabled || start.isPending}>
          Share
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
