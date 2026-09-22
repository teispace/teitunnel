import { useEffect } from "react";
import { useSettings } from "@/features/settings";
import { applyTheme } from "./theme";

/** Keeps this window's appearance in step with the saved setting (any window may change it). */
export function ThemeSync() {
  const theme = useSettings().data?.theme;
  useEffect(() => {
    if (theme) void applyTheme(theme);
  }, [theme]);
  return null;
}
