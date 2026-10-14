import { useQuery } from "@tanstack/react-query";
import { commands, type CommandError } from "./bindings";

export function useSystemChecks() {
  return useQuery({
    queryKey: ["system_checks"],
    queryFn: () => commands.systemChecks(),
  });
}

/** Turns a rejected command (a `CommandError`) or any other throwable into text. */
export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as CommandError).message);
  }
  return String(error);
}
