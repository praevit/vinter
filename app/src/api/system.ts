import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";

export function useSystemChecks() {
  return useQuery({
    queryKey: ["system_checks"],
    queryFn: () => commands.systemChecks(),
  });
}
