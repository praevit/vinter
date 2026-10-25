import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";

/** The user's compatibility notes, for every bottle. */
export function useCompatNotes() {
  const query = useQuery({ queryKey: ["compat-notes"], queryFn: () => commands.compatNotesList() });
  return { notes: query.data ?? [] };
}
