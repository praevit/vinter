import { useQueries } from "@tanstack/react-query";
import { commands, type BottleListItem, type ProgramView } from "./bindings";
import { useBottles } from "./bottles";
import { useInstalledRuntimes } from "./runtimes";

/** A program and the bottle it lives in. */
export type AppEntry = { bottle: BottleListItem; program: ProgramView; needsRosetta: boolean };

/** Every program in every ready bottle (shares the per-bottle program lists' cache). */
export function useAllPrograms(): AppEntry[] {
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const ready = bottles?.filter((b) => b.status === "ready" && b.program_count > 0) ?? [];
  const lists = useQueries({
    queries: ready.map((b) => ({
      queryKey: ["bottles", b.id, "programs"],
      queryFn: () => commands.programsList(b.id),
    })),
  });
  return ready.flatMap((bottle, i) => {
    const needsRosetta = runtimes?.find((r) => r.id === bottle.runtime)?.needs_rosetta ?? false;
    return (lists[i]?.data ?? []).map((program) => ({ bottle, program, needsRosetta }));
  });
}

/** The most recently opened programs, newest first. */
export function recentPrograms(apps: AppEntry[], count: number): AppEntry[] {
  return apps
    .filter((a) => a.program.last_launched)
    .sort((a, b) => b.program.last_launched!.localeCompare(a.program.last_launched!))
    .slice(0, count);
}

/** "5 min", "2 h 10 min": how long ago an RFC 3339 time was. */
export function elapsed(since: string, now: number): string {
  const minutes = Math.max(0, Math.floor((now - Date.parse(since)) / 60000));
  if (minutes < 1) return "less than a minute";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
}
