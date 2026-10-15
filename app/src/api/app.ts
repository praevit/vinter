import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";

export function useAppInfo() {
  return useQuery({ queryKey: ["app-info"], queryFn: () => commands.appInfo(), staleTime: Infinity });
}

/** "Add to Applications" (macOS) or "Add to app menu" (Linux). */
export function pinLabel(os: string | undefined, pinned: boolean) {
  const where = os === "macos" ? "Applications" : "app menu";
  return pinned ? `Remove from ${where}` : `Add to ${where}`;
}
