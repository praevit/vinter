import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type DiscoveredProgram } from "./bindings";

export const installerJobKey = (bottleId: string) => `installer:${bottleId}`;

export function usePrograms(bottleId: string) {
  return useQuery({
    queryKey: ["bottles", bottleId, "programs"],
    queryFn: () => commands.programsList(bottleId),
  });
}

export function useAddPrograms(bottleId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (programs: DiscoveredProgram[]) =>
      commands.programsAdd(
        bottleId,
        programs.map((p) => ({ name: p.name, exe: p.exe, args: p.args, icon: p.icon, pinned: false })),
      ),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bottles"] }),
  });
}

export function useRemoveProgram(bottleId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (index: number) => commands.programRemove(bottleId, index),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bottles"] }),
  });
}

export function useLogs(bottleId: string) {
  return useQuery({
    queryKey: ["bottles", bottleId, "logs"],
    queryFn: () => commands.logsList(bottleId),
  });
}

export function useLog(bottleId: string, logId: string | undefined, live: boolean) {
  return useQuery({
    queryKey: ["bottles", bottleId, "logs", logId],
    queryFn: () => commands.logRead(bottleId, logId!),
    enabled: !!logId && !live,
  });
}
