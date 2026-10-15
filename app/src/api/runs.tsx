import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { commands, events, type RunInfo, type Stream } from "./bindings";
import { useToasts } from "./toasts";

/** Lines kept in memory per run; the full output is in the run's log file. */
const MAX_LINES = 2000;

export type Run = {
  info: RunInfo;
  lines: { line: string; stream: Stream }[];
  /** Set when the program has exited: its exit code, or null if it was stopped. */
  exit?: { code: number | null };
};

type Runs = {
  runs: Run[];
  /** Records a run started from the UI (its events may already be arriving). */
  add: (info: RunInfo) => void;
};

const RunsContext = createContext<Runs | null>(null);

/** Tracks programs started from Vinter and their output, for the whole app. */
export function RunsProvider({ children }: { children: ReactNode }) {
  const [runs, setRuns] = useState<Record<string, Run>>({});
  const toasts = useToasts();

  const add = useCallback((info: RunInfo) => {
    setRuns((r) => ({ ...r, [info.run_id]: { ...(r[info.run_id] ?? { lines: [] }), info } }));
  }, []);

  useEffect(() => {
    // Runs started before this window loaded (e.g. after a reload).
    commands.runsList().then((list) => list.forEach(add));
    const log = events.runLog.listen(({ payload: p }) =>
      setRuns((r) => {
        const run = r[p.run_id] ?? { info: undefined as unknown as RunInfo, lines: [] };
        const lines = [...run.lines, { line: p.line, stream: p.stream }].slice(-MAX_LINES);
        return { ...r, [p.run_id]: { ...run, lines } };
      }),
    );
    const exit = events.runExit.listen(({ payload: p }) =>
      setRuns((r) => {
        const run = r[p.run_id];
        if (!run) return r;
        if (p.code !== null && p.code !== 0) {
          toasts.show(`${run.info.name} exited with code ${p.code}. Its log is in the bottle's Logs tab.`);
        }
        return { ...r, [p.run_id]: { ...run, exit: { code: p.code } } };
      }),
    );
    return () => {
      log.then((u) => u());
      exit.then((u) => u());
    };
  }, [add, toasts]);

  const list = Object.values(runs).filter((r) => r.info);
  return <RunsContext value={{ runs: list, add }}>{children}</RunsContext>;
}

export function useRuns(): Runs {
  const runs = useContext(RunsContext);
  if (!runs) throw new Error("useRuns must be used inside <RunsProvider>");
  return runs;
}

/** The latest run of one program in a bottle, if any. */
export function latestRun(runs: Run[], bottleId: string, index: number): Run | undefined {
  return runs
    .filter((r) => r.info.bottle_id === bottleId && r.info.program_index === index)
    .sort((a, b) => b.info.started_at.localeCompare(a.info.started_at))[0];
}
