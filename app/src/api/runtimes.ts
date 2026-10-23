import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type EngineKind } from "./bindings";

/** What people see for an engine (matches `EngineKind::label` in Rust). */
export const ENGINE_LABELS: Record<EngineKind, string> = {
  "x86_64-rosetta": "Intel (Rosetta)",
  "arm64-fex": "Apple Silicon",
  "x86_64-native": "x86_64",
  "arm64-fex-linux": "ARM64",
};

export function useAvailableRuntimes() {
  return useQuery({
    queryKey: ["runtimes", "available"],
    queryFn: () => commands.runtimesAvailable(),
    staleTime: Infinity,
  });
}

/** The ARM64 engine is shown as "Preview" until the owner removes that label. */
export const isPreviewEngine = (engine: EngineKind) => engine === "arm64-fex";

/** A runtime's name for pickers, e.g. "Apple Silicon Wine 11.10 (Preview)". */
export function runtimeLabel(r: { name: string; engine: EngineKind }): string {
  return isPreviewEngine(r.engine) ? `${r.name} (Preview)` : r.name;
}

/** Id of the runtime new bottles use unless the user picks another. */
export function useDefaultRuntime() {
  return useQuery({ queryKey: ["runtimes", "default"], queryFn: () => commands.runtimeDefault() });
}

export function useInstalledRuntimes() {
  return useQuery({
    queryKey: ["runtimes", "installed"],
    queryFn: () => commands.runtimesInstalled(),
  });
}

export function useRemoveRuntime() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => commands.runtimeRemove(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["runtimes"] }),
  });
}

/** Job key for a runtime install, see `useJobs`. */
export const runtimeJobKey = (id: string) => `runtime:${id}`;

export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}
