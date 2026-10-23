import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type ExperimentalFlag } from "./bindings";

export function useExperimentalFlags() {
  return useQuery({ queryKey: ["experimental"], queryFn: () => commands.experimentalList() });
}

/** Whether one experimental feature is on. False until the list has loaded. */
export function useExperimental(flag: ExperimentalFlag): boolean {
  const { data } = useExperimentalFlags();
  return data?.find((f) => f.flag === flag)?.enabled ?? false;
}

export function useSetExperimental() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ flag, enabled }: { flag: ExperimentalFlag; enabled: boolean }) =>
      commands.experimentalSet(flag, enabled),
    // Flags can change what any page shows (runtimes, readiness...): reload everything.
    onSettled: () => queryClient.invalidateQueries(),
  });
}
