import { type ReactNode, useId } from "react";
import { cn } from "@/lib/cn";

export interface FieldControlProps {
  id: string;
  "aria-describedby"?: string;
  "aria-invalid"?: true;
}

interface FieldProps {
  label: string;
  help?: ReactNode;
  error?: string | null | undefined;
  className?: string;
  /** Receives the ids and ARIA wiring for the control. */
  children: (control: FieldControlProps) => ReactNode;
}

/** Label + control + help or error text, with ARIA wiring handled. */
export function Field({ label, help, error, className, children }: FieldProps) {
  const id = useId();
  const messageId = `${id}-message`;
  const message = error ?? help;
  const control: FieldControlProps = { id };
  if (message) control["aria-describedby"] = messageId;
  if (error) control["aria-invalid"] = true;
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <label htmlFor={id} className="text-body text-primary">
        {label}
      </label>
      {children(control)}
      {message ? (
        <p id={messageId} className={cn("text-callout", error ? "text-error" : "text-secondary")}>
          {message}
        </p>
      ) : null}
    </div>
  );
}
