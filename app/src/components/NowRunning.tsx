import { useEffect, useRef, useState } from "react";
import { AppWindow, ScrollText, Square } from "lucide-react";
import { elapsed, useAllPrograms } from "../api/allPrograms";
import { commands } from "../api/bindings";
import { isMac } from "../api/contextMenu";
import { errorMessage } from "../api/errors";
import { runningRuns, useRuns, type Run } from "../api/runs";
import { useToasts } from "../api/toasts";
import { Dialog } from "./Dialog";
import { ProgramIcon } from "./InstallDialog";
import { Button, subtle } from "./ui";

/** Home page strip: programs running right now, with Show, Stop and their log. */
export function NowRunning() {
  const { runs } = useRuns();
  const apps = useAllPrograms();
  const toasts = useToasts();
  const [now, setNow] = useState(() => Date.now());
  const [logOf, setLogOf] = useState<string>();
  const running = runningRuns(runs);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);

  if (running.length === 0) return null;

  const show = async (run: Run) => {
    try {
      if (!(await commands.programShow(run.info.run_id))) {
        toasts.show(`Vinter could not find ${run.info.name}'s window. Look for it in the Dock.`, "info");
      }
    } catch (e) {
      toasts.show(errorMessage(e));
    }
  };

  const logRun = running.find((r) => r.info.run_id === logOf);

  return (
    <section className="flex flex-col gap-2" aria-label="Now running">
      <h2 className="text-sm font-medium">Now running</h2>
      <ul className="flex flex-col divide-y divide-frost-200 rounded-xl border border-frost-200 bg-white dark:divide-slate-deep-700 dark:border-slate-deep-700 dark:bg-slate-deep-900">
        {running.map((run) => {
          const app = apps.find(
            (a) => a.bottle.id === run.info.bottle_id && a.program.index === run.info.program_index,
          );
          return (
            <li key={run.info.run_id} className="flex items-center gap-3 px-3 py-2">
              {app ? (
                <ProgramIcon src={app.program.icon_data_url} size="size-8" />
              ) : (
                <AppWindow className="size-8 text-frost-400" />
              )}
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm font-medium">{run.info.name}</span>
                <span className={`${subtle} truncate`}>
                  {app?.bottle.name ?? "Bottle"} · running for {elapsed(run.info.started_at, now)}
                </span>
              </div>
              {isMac && (
                <Button onClick={() => show(run)} title="Bring its window to the front">
                  <AppWindow className="size-3.5" /> Show
                </Button>
              )}
              <Button onClick={() => commands.programStop(run.info.run_id)}>
                <Square className="size-3.5" /> Stop
              </Button>
              <Button aria-label={`Log of ${run.info.name}`} title="Log" onClick={() => setLogOf(run.info.run_id)}>
                <ScrollText className="size-3.5" />
              </Button>
            </li>
          );
        })}
      </ul>
      {logRun && <LiveLog run={logRun} onClose={() => setLogOf(undefined)} />}
    </section>
  );
}

/** A running program's output so far, following new lines. */
function LiveLog({ run, onClose }: { run: Run; onClose: () => void }) {
  const view = useRef<HTMLPreElement>(null);
  const text = run.lines.map((l) => l.line).join("\n");
  useEffect(() => {
    view.current?.scrollTo({ top: view.current.scrollHeight });
  }, [text]);
  return (
    <Dialog onClose={onClose}>
      <div className="flex flex-col gap-3">
        <h2 className="font-semibold">{run.info.name}</h2>
        <pre
          ref={view}
          className="h-72 overflow-auto rounded-lg bg-frost-100 p-3 text-xs whitespace-pre-wrap select-text dark:bg-slate-deep-950"
        >
          {text || "No output yet."}
        </pre>
        <p className={subtle}>The full log is in the bottle's Logs tab.</p>
        <div className="flex justify-end">
          <Button onClick={onClose}>Close</Button>
        </div>
      </div>
    </Dialog>
  );
}
