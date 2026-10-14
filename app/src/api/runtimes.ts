import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands } from "./bindings";

export function useAvailableRuntimes() {
  return useQuery({
    queryKey: ["runtimes", "available"],
    queryFn: () => commands.runtimesAvailable(),
    staleTime: Infinity,
  });
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
