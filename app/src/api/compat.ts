import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";
import { useExperimental } from "./experimental";

/** Compatibility notes, or none while the `compat-notes` flag is off. */
export function useCompatNotes() {
  const enabled = useExperimental("compat-notes");
  const query = useQuery({ queryKey: ["compat-notes"], queryFn: () => commands.compatNotesList(), enabled });
  return { enabled, notes: enabled ? (query.data ?? []) : [] };
}
