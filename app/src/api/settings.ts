import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type Settings } from "./bindings";

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: () => commands.settingsGet() });
}

export function useSaveSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: Settings) => commands.settingsSet(settings),
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      // The default runtime for new bottles follows the Settings choice.
      await queryClient.invalidateQueries({ queryKey: ["runtimes", "default"] });
    },
  });
}
