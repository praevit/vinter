import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { Appearance } from "./bindings";
import { useSettings } from "./settings";

const root = document.documentElement;
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
let chosen: Appearance = "system";

export const isMac = /Mac OS X|Macintosh/.test(navigator.userAgent);

function apply() {
  root.classList.toggle("dark", chosen === "dark" || (chosen === "system" && systemDark.matches));
}

/**
 * Sets the platform classes on <html> (`platform-macos`, `platform-linux`,
 * and `vibrancy` for the main window on macOS) and follows the system's
 * light or dark mode. Runs once, before the first render.
 */
export function startTheme({ vibrancy }: { vibrancy: boolean }) {
  root.classList.add(isMac ? "platform-macos" : "platform-linux");
  root.classList.toggle("vibrancy", isMac && vibrancy);
  apply();
  systemDark.addEventListener("change", apply);
}

/**
 * Applies the Appearance setting in this window. Rust sets the windows'
 * theme too (so windows that cannot read settings follow it); this also
 * covers webviews that do not pass it on. Rereads the setting when the
 * window comes forward or its theme changes, since it may have changed in
 * another window.
 */
export function useAppearance() {
  const { data } = useSettings();
  const queryClient = useQueryClient();
  const appearance = data?.appearance;

  useEffect(() => {
    if (!appearance) return;
    chosen = appearance;
    apply();
  }, [appearance]);

  useEffect(() => {
    const refresh = () => void queryClient.invalidateQueries({ queryKey: ["settings"] });
    window.addEventListener("focus", refresh);
    // Rust changes every window's theme when the setting changes.
    systemDark.addEventListener("change", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      systemDark.removeEventListener("change", refresh);
    };
  }, [queryClient]);
}
