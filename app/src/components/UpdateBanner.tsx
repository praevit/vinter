import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Sparkles, X } from "lucide-react";
import { commands, events } from "../api/bindings";
import { useAppInfo } from "../api/app";
import { isRunning, useJobs } from "../api/jobs";
import { useSettings } from "../api/settings";
import { ProgressBar } from "./ProgressBar";
import type { Route } from "./Sidebar";
import { Button, ErrorText } from "./ui";

const UPDATE_JOB = "app-update";
const DISMISSED_KEY = "vinter.dismissedUpdates";

/** "Later" hides an update until a newer one comes; remembered per viewer. */
function loadDismissed(): string[] {
  try {
    return JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
  } catch {
    return [];
  }
}

function saveDismissed(keys: string[]) {
  try {
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(keys));
  } catch {
    // Not remembered; the banner just comes back next time.
  }
}

/**
 * What the automatic update checks found: a newer Vinter (install from
 * here) and newer runtimes (a link to the Runtimes page). Hidden with
 * "Check for updates automatically" off.
 */
export function UpdateBanner({ route, onNavigate }: { route: Route; onNavigate: (r: Route) => void }) {
  const queryClient = useQueryClient();
  const { data: settings } = useSettings();
  const { data: info } = useAppInfo();
  const jobs = useJobs();
  const job = jobs.job(UPDATE_JOB);
  const [dismissed, setDismissed] = useState(loadDismissed);
  const { data: status } = useQuery({ queryKey: ["update-status"], queryFn: () => commands.updateStatus() });
  const { data: runtimeUpdates } = useQuery({
    queryKey: ["runtimes", "updates"],
    queryFn: () => commands.runtimeUpdates(),
  });

  useEffect(() => {
    const found = events.appUpdates.listen(() => {
      void queryClient.invalidateQueries({ queryKey: ["update-status"] });
      void queryClient.invalidateQueries({ queryKey: ["runtimes", "updates"] });
    });
    return () => void found.then((unlisten) => unlisten());
  }, [queryClient]);

  if (!settings?.check_for_updates) return null;

  const dismiss = (key: string) => {
    const next = [...dismissed, key];
    setDismissed(next);
    saveDismissed(next);
  };

  const app = status?.app;
  const appKey = app && `vinter-${app.version}`;
  const showApp = app && appKey && (!dismissed.includes(appKey) || isRunning(job));

  const runtimes = runtimeUpdates ?? [];
  const runtimesKey = runtimes.map((u) => u.newer.id).sort().join(",");
  const showRuntimes = runtimes.length > 0 && route.page !== "runtimes" && !dismissed.includes(runtimesKey);
  const runtimeText =
    runtimes.length === 1
      ? `${runtimes[0].newer.name}${runtimes[0].newer_build !== null ? ` build ${runtimes[0].newer_build}` : ""} is available.`
      : `Updates for ${runtimes.length} runtimes are available.`;

  if (!showApp && !showRuntimes) return null;

  return (
    <div className="flex flex-col gap-2 px-8 pt-6">
      {showApp && (
        <Bar onClose={isRunning(job) ? undefined : () => dismiss(appKey)}>
          <Sparkles className="size-4 shrink-0 text-wine-500" />
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span>
              <span className="font-medium">Vinter {app.version} is available.</span> You have{" "}
              {info?.version ?? "an older version"}.
            </span>
            {isRunning(job) && (
              <>
                <ProgressBar percent={job?.percent ?? null} />
                <span className="text-xs opacity-70">{job?.step}… Vinter restarts when it is done.</span>
              </>
            )}
            {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}
          </div>
          {!isRunning(job) && (
            <Button variant="primary" onClick={() => jobs.start(UPDATE_JOB, () => commands.updateInstall())}>
              <Download className="size-4" /> Install and restart
            </Button>
          )}
        </Bar>
      )}
      {showRuntimes && (
        <Bar onClose={() => dismiss(runtimesKey)}>
          <Download className="size-4 shrink-0 text-wine-500" />
          <span className="min-w-0 flex-1">{runtimeText}</span>
          <Button onClick={() => onNavigate({ page: "runtimes" })}>Show runtimes</Button>
        </Bar>
      )}
    </div>
  );
}

function Bar({ children, onClose }: { children: React.ReactNode; onClose?: () => void }) {
  return (
    <div className="mx-auto flex w-full max-w-3xl items-center gap-3 rounded-xl border border-wine-500/30 bg-wine-500/5 px-4 py-2.5 text-sm">
      {children}
      {onClose && (
        <button
          type="button"
          onClick={onClose}
          aria-label="Later"
          title="Later"
          className="rounded p-1 opacity-60 hover:bg-frost-100 hover:opacity-100 dark:hover:bg-slate-deep-800"
        >
          <X className="size-4" />
        </button>
      )}
    </div>
  );
}
