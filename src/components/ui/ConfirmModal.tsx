import React, { useEffect, useCallback } from "react";
import { AlertTriangle, AlertCircle, Loader2, X } from "lucide-react";

export interface ConfirmModalProps {
  isOpen: boolean;
  title: string;
  description: string;
  details?: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: "danger" | "warning" | "default";
  isLoading?: boolean;
  onConfirm: () => void | Promise<void>;
  onClose: () => void;
}

export const ConfirmModal: React.FC<ConfirmModalProps> = ({
  isOpen,
  title,
  description,
  details,
  confirmLabel = "Delete",
  cancelLabel = "Cancel",
  variant = "danger",
  isLoading = false,
  onConfirm,
  onClose,
}) => {
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!isOpen) return;
      if (e.key === "Escape" && !isLoading) {
        onClose();
      } else if (e.key === "Enter" && !isLoading) {
        e.preventDefault();
        onConfirm();
      }
    },
    [isOpen, isLoading, onClose, onConfirm]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!isOpen) return null;

  const isDanger = variant === "danger";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop with vibrancy blur */}
      <div
        className="fixed inset-0 bg-black/65 backdrop-blur-md transition-opacity animate-in fade-in duration-200"
        onClick={isLoading ? undefined : onClose}
      />

      {/* Modal Dialog Card */}
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        className="relative w-full max-w-md rounded-2xl bg-zinc-900/95 border border-zinc-750 p-6 shadow-2xl shadow-black/80 backdrop-blur-xl animate-in zoom-in-95 duration-150 z-10 space-y-5"
      >
        {/* Subtle accent highlight line */}
        <div className="absolute top-0 inset-x-0 h-px bg-gradient-to-r from-transparent via-zinc-600/40 to-transparent" />

        {/* Header Row with Icon and Close Button */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3.5">
            <div
              className={`p-3 rounded-xl border flex items-center justify-center shrink-0 ${
                isDanger
                  ? "bg-rose-950/40 border-rose-800/60 text-rose-400"
                  : "bg-amber-950/40 border-amber-800/60 text-amber-400"
              }`}
            >
              {isDanger ? (
                <AlertTriangle className="w-5 h-5" />
              ) : (
                <AlertCircle className="w-5 h-5" />
              )}
            </div>
            <div>
              <h3
                id="confirm-dialog-title"
                className="text-base font-semibold text-zinc-100 tracking-tight"
              >
                {title}
              </h3>
              <p className="text-xs text-zinc-400 mt-0.5 leading-relaxed">
                {description}
              </p>
            </div>
          </div>

          {!isLoading && (
            <button
              onClick={onClose}
              className="text-zinc-500 hover:text-zinc-300 p-1.5 rounded-lg hover:bg-zinc-800/60 transition cursor-pointer"
              aria-label="Close dialog"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>

        {/* Optional Metadata / Target details */}
        {details && (
          <div className="p-3 rounded-xl bg-zinc-950/80 border border-zinc-800/80 text-xs text-zinc-300 font-mono break-all">
            {details}
          </div>
        )}

        {/* Action Buttons */}
        <div className="flex items-center justify-end gap-3 pt-2">
          <button
            type="button"
            disabled={isLoading}
            onClick={onClose}
            className="px-4 py-2 rounded-xl text-xs font-medium text-zinc-300 bg-zinc-800/80 hover:bg-zinc-750 border border-zinc-700/60 transition disabled:opacity-50 cursor-pointer"
          >
            {cancelLabel}
          </button>

          <button
            type="button"
            disabled={isLoading}
            onClick={onConfirm}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-semibold shadow-lg transition cursor-pointer disabled:opacity-50 ${
              isDanger
                ? "bg-rose-600 hover:bg-rose-500 text-white shadow-rose-950/50 border border-rose-500/60"
                : "bg-amber-600 hover:bg-amber-500 text-white shadow-amber-950/50 border border-amber-500/60"
            }`}
          >
            {isLoading && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            <span>{confirmLabel}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
