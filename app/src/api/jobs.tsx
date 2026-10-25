import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { announce } from "./announce";
import { events } from "./bindings";
import { errorMessage } from "./errors";

export type JobState = {
  step: string;
  message: string;
  percent: number | null;
  /** Set once the job has finished. */
  done?: { ok: boolean; error: string | null };
};

type Jobs = {
  /** Job state for a key such as `runtime:<id>`, if one was started. */
  job: (key: string) => JobState | undefined;
  /** Starts a job (a command that returns a job id) under a key. */
  start: (key: string, run: () => Promise<string>) => Promise<void>;
};

const JobsContext = createContext<Jobs | null>(null);

/** Listens to job events for the whole app, so progress survives page changes. */
export function JobsProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [byId, setById] = useState<Record<string, JobState>>({});
  const [keys, setKeys] = useState<Record<string, string>>({});

  useEffect(() => {
    const progress = events.jobProgress.listen(({ payload: p }) =>
      setById((jobs) =>
        jobs[p.job_id]?.done
          ? jobs
          : { ...jobs, [p.job_id]: { step: p.step, message: p.message, percent: p.percent } },
      ),
    );
    const done = events.jobDone.listen(({ payload: d }) => {
      setById((jobs) => ({
        ...jobs,
        [d.job_id]: {
          step: jobs[d.job_id]?.step ?? "",
          message: jobs[d.job_id]?.message ?? "",
          percent: d.ok ? 100 : (jobs[d.job_id]?.percent ?? null),
          done: { ok: d.ok, error: d.error },
        },
      }));
      // Failures show a toast, which screen readers already read out.
      if (d.ok) announce("Done.");
      // Any finished job may have changed what is installed.
      queryClient.invalidateQueries();
    });
    return () => {
      progress.then((unlisten) => unlisten());
      done.then((unlisten) => unlisten());
    };
  }, [queryClient]);

  const start = useCallback(async (key: string, run: () => Promise<string>) => {
    setKeys((k) => ({ ...k, [key]: "" }));
    try {
      const id = await run();
      setKeys((k) => ({ ...k, [key]: id }));
    } catch (e) {
      // The command itself failed before a job started: record it as a failed job.
      const id = `failed:${key}:${Date.now()}`;
      setById((jobs) => ({
        ...jobs,
        [id]: { step: "", message: "", percent: null, done: { ok: false, error: errorMessage(e) } },
      }));
      setKeys((k) => ({ ...k, [key]: id }));
    }
  }, []);

  const job = useCallback(
    (key: string): JobState | undefined => {
      const id = keys[key];
      if (id === undefined) return undefined;
      // "" means the command was sent and the job id has not come back yet.
      return byId[id] ?? { step: "Starting", message: "", percent: null };
    },
    [keys, byId],
  );

  return <JobsContext value={{ job, start }}>{children}</JobsContext>;
}

export function useJobs(): Jobs {
  const jobs = useContext(JobsContext);
  if (!jobs) throw new Error("useJobs must be used inside <JobsProvider>");
  return jobs;
}

/** True while a job has started and not finished. */
export function isRunning(job: JobState | undefined): boolean {
  return !!job && !job.done;
}
