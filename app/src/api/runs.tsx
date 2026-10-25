import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { commands, events, type RunInfo, type Stream } from "./bindings";
import { logRoute, navigate } from "./navigate";
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
  /**
   * Records a run (its events may already be arriving). Safe to call twice.
   * `quiet` runs get no toast when they fail (the "Fix it" steps show it themselves).
   */
  add: (info: RunInfo, options?: { quiet?: boolean }) => void;
};

const RunsContext = createContext<Runs | null>(null);

/** Tracks programs started from Vinter and their output, for the whole app. */
export function RunsProvider({ children }: { children: ReactNode }) {
  const [runs, setRuns] = useState<Record<string, Run>>({});
  const toasts = useToasts();
  const queryClient = useQueryClient();
  // Read when a run exits, outside the state updater (which may run twice).
  const infos = useRef(new Map<string, RunInfo>());
  const quiet = useRef(new Set<string>());

  const add = useCallback((info: RunInfo, options?: { quiet?: boolean }) => {
    infos.current.set(info.run_id, info);
    if (options?.quiet) quiet.current.add(info.run_id);
    setRuns((r) => ({ ...r, [info.run_id]: { ...(r[info.run_id] ?? { lines: [] }), info } }));
  }, []);

  useEffect(() => {
    // Runs started before this window loaded (e.g. after a reload).
    commands.runsList().then((list) => list.forEach((info) => add(info)));
    // Runs started anywhere: here, the menu bar, a launcher or a link.
    const started = events.runStarted.listen(({ payload: { info } }) => {
      add(info);
      // Its "last used" time changed.
      void queryClient.invalidateQueries({ queryKey: ["bottles", info.bottle_id, "programs"] });
    });
    const log = events.runLog.listen(({ payload: p }) =>
      setRuns((r) => {
        const run = r[p.run_id] ?? { info: undefined as unknown as RunInfo, lines: [] };
        const lines = [...run.lines, { line: p.line, stream: p.stream }].slice(-MAX_LINES);
        return { ...r, [p.run_id]: { ...run, lines } };
      }),
    );
    const exit = events.runExit.listen(({ payload: p }) => {
      setRuns((r) => {
        const run = r[p.run_id];
        if (!run) return r;
        return { ...r, [p.run_id]: { ...run, exit: { code: p.code } } };
      });
      const info = infos.current.get(p.run_id);
      if (info && p.code !== null && p.code !== 0 && !quiet.current.has(p.run_id)) {
        void exitToast(info, p.code);
      }
    });
    // What the log doctor makes of a failed run, with a way to its log.
    const exitToast = async (info: RunInfo, code: number) => {
      const top = await commands.logDiagnose(info.bottle_id, info.log_id).then(
        (f) => f[0],
        () => undefined,
      );
      const open = () => navigate(logRoute(info.bottle_id, info.log_id));
      toasts.show(
        top ? `${info.name} quit: ${top.title}.` : `${info.name} exited with code ${code}.`,
        "error",
        { label: top ? "See how to fix it" : "Show log", onClick: open },
      );
    };
    return () => {
      started.then((u) => u());
      log.then((u) => u());
      exit.then((u) => u());
    };
  }, [add, toasts, queryClient]);

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

/** Runs that have not exited yet, oldest first. */
export function runningRuns(runs: Run[]): Run[] {
  return runs.filter((r) => !r.exit).sort((a, b) => a.info.started_at.localeCompare(b.info.started_at));
}
