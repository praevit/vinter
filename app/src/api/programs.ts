import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type DiscoveredProgram, type ProgramDef_Serialize as ProgramDef } from "./bindings";
import { errorMessage } from "./errors";
import { useToasts } from "./toasts";

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

/** Removes a program's tile, with "Undo" in a toast that puts it back. */
export function useRemoveProgram(bottleId: string) {
  const queryClient = useQueryClient();
  const toasts = useToasts();
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["bottles"] });

  const putBack = async (program: ProgramDef) => {
    try {
      // Added unpinned, then pinned, so its launcher comes back too.
      const bottle = await commands.programsAdd(bottleId, [{ ...program, pinned: false }]);
      const index = bottle.programs.findIndex((p) => p.exe.toLowerCase() === program.exe.toLowerCase());
      if (program.pinned && index >= 0) await commands.programSetPinned(bottleId, index, true);
    } catch (e) {
      toasts.show(`Could not put ${program.name} back: ${errorMessage(e)}`);
    }
    await refresh();
  };

  return useMutation({
    mutationFn: async (index: number) => {
      const program = (await commands.bottleGet(bottleId)).programs[index];
      await commands.programRemove(bottleId, index);
      return program;
    },
    onSuccess: (program) => {
      if (!program) return;
      toasts.show(`Removed ${program.name}.`, "info", { label: "Undo", onClick: () => void putBack(program) });
    },
    onSettled: refresh,
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
