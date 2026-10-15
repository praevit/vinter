import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, ExternalLink, RotateCcw, Trash2 } from "lucide-react";
import {
  commands,
  type Flavor,
  type Runtime,
  type RuntimeInfo_Serialize as RuntimeInfo,
} from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { useEnsureRosetta } from "../api/rosetta";
import {
  formatBytes,
  runtimeJobKey,
  useAvailableRuntimes,
  useInstalledRuntimes,
  useRemoveRuntime,
} from "../api/runtimes";
import { ProgressBar } from "../components/ProgressBar";
import { Button, ErrorText } from "../components/ui";

const FLAVORS: Record<Flavor, string> = {
  stable: "Stable",
  devel: "Development",
  staging: "Staging",
  gptk: "GPTK",
};

export function Runtimes() {
  const available = useAvailableRuntimes();
  const installed = useInstalledRuntimes();
  const installedIds = new Set(installed.data?.map((r) => r.id));
  const notInstalled = available.data?.filter((r) => !installedIds.has(r.id)) ?? [];

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 px-8 py-10">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Runtimes</h1>
        <p className="mt-1 text-sm text-slate-deep-700/70 dark:text-frost-300/70">
          Wine builds that run your Windows programs. Vinter downloads them from their publishers
          and checks every download's fingerprint before installing it.
        </p>
      </header>

      <Updates />

      <Section title="Installed">
        {installed.error ? (
          <Empty>Could not list runtimes: {errorMessage(installed.error)}</Empty>
        ) : installed.data?.length === 0 ? (
          <Empty>No runtimes installed yet. Install one below to get started.</Empty>
        ) : (
          installed.data?.map((r) => <InstalledRow key={r.id} runtime={r} />)
        )}
      </Section>

      {notInstalled.length > 0 && (
        <Section title="Available">
          {notInstalled.map((r) => (
            <AvailableRow key={r.id} runtime={r} />
          ))}
        </Section>
      )}
    </div>
  );
}

/** Newer builds of installed runtimes, with an Update button. */
function Updates() {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const queryClient = useQueryClient();
  const { data: updates } = useQuery({ queryKey: ["runtimes", "updates"], queryFn: () => commands.runtimeUpdates() });
  if (!updates?.length) return null;

  return (
    <Section title="Updates">
      {updates.map((u) => {
        const key = `runtime-update:${u.installed_id}`;
        const job = jobs.job(key);
        const update = async () => {
          if (!(await ensureRosetta(u.newer.needs_rosetta))) return;
          await jobs.start(key, () => commands.runtimeUpdate(u.installed_id));
          await queryClient.invalidateQueries({ queryKey: ["runtimes"] });
        };
        return (
          <li key={u.installed_id} className="flex flex-col gap-2 px-4 py-3">
            <div className="flex items-center gap-4">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="text-sm font-medium">{u.newer.name} is available</span>
                <span className="text-xs text-slate-deep-700/60 dark:text-frost-300/60">
                  You have {u.installed_version}.{" "}
                  {u.bottles.length > 0 ? `Moves ${u.bottles.join(", ")} onto it.` : "No bottles use it yet."}{" "}
                  {formatBytes(u.newer.size)} download.
                </span>
              </div>
              <Button variant="primary" disabled={isRunning(job)} onClick={update}>
                <Download className="size-4" /> Update
              </Button>
            </div>
            {isRunning(job) && <ProgressBar percent={job?.percent ?? null} />}
            {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}
          </li>
        );
      })}
    </Section>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-xs font-semibold tracking-wide text-frost-400 uppercase">{title}</h2>
      <ul className="divide-y divide-frost-200 rounded-xl border border-frost-200 bg-white dark:divide-slate-deep-700 dark:border-slate-deep-700 dark:bg-slate-deep-900">
        {children}
      </ul>
    </section>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <li className="px-4 py-6 text-center text-sm text-frost-400">{children}</li>;
}

function Title({ name, flavor }: { name: string; flavor: Flavor }) {
  return (
    <div className="flex items-center gap-2">
      <span className="font-medium">{name}</span>
      <span className="rounded bg-frost-100 px-1.5 py-0.5 text-[11px] font-medium text-slate-deep-700 dark:bg-slate-deep-800 dark:text-frost-300">
        {FLAVORS[flavor]}
      </span>
    </div>
  );
}

const subtle = "text-xs text-slate-deep-700/60 dark:text-frost-300/60";
const button =
  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-sm disabled:opacity-50 hover:bg-frost-100 dark:hover:bg-slate-deep-800";

function InstalledRow({ runtime: r }: { runtime: Runtime }) {
  const remove = useRemoveRuntime();
  const [confirming, setConfirming] = useState(false);

  return (
    <li className="flex items-center gap-4 px-4 py-3">
      <div className="min-w-0 flex-1">
        <Title name={r.name} flavor={r.flavor} />
        <div className={subtle}>
          {formatBytes(r.size)} on disk · installed {new Date(r.installed_at).toLocaleDateString()}
        </div>
        {remove.error && (
          <div className="text-xs text-wine-500 dark:text-rose-400">
            {errorMessage(remove.error)}
          </div>
        )}
      </div>
      <button
        type="button"
        className={`${button} text-slate-deep-700 dark:text-frost-300`}
        onClick={() => commands.runtimeOpenLink(r.id, "source")}
        title="Wine is free software under the LGPL. This opens the exact source code of this build."
      >
        <ExternalLink className="size-4" /> Source
      </button>
      {confirming ? (
        <button
          type="button"
          className={`${button} bg-wine-500 text-white hover:bg-wine-600 dark:hover:bg-wine-600`}
          disabled={remove.isPending}
          onBlur={() => setConfirming(false)}
          onClick={() => remove.mutate(r.id)}
          autoFocus
        >
          <Trash2 className="size-4" /> {remove.isPending ? "Removing…" : "Confirm"}
        </button>
      ) : (
        <button
          type="button"
          className={`${button} text-wine-500 dark:text-rose-300`}
          onClick={() => setConfirming(true)}
        >
          <Trash2 className="size-4" /> Remove
        </button>
      )}
    </li>
  );
}

function AvailableRow({ runtime: r }: { runtime: RuntimeInfo }) {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const job = jobs.job(runtimeJobKey(r.id));
  const running = isRunning(job);
  const error = job?.done && !job.done.ok ? job.done.error : null;
  const install = async () => {
    // Downloading works without Rosetta, so skipping it still installs.
    await ensureRosetta(r.needs_rosetta, { optional: true });
    jobs.start(runtimeJobKey(r.id), () => commands.runtimeInstall(r.id));
  };

  return (
    <li className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <Title name={r.name} flavor={r.flavor} />
          <div className={subtle}>
            {formatBytes(r.size)} download · {r.license} ·{" "}
            <button
              type="button"
              className="underline-offset-2 hover:underline"
              onClick={() => commands.runtimeOpenLink(r.id, "release")}
            >
              release page
            </button>
          </div>
        </div>
        {!running && (
          <button
            type="button"
            onClick={install}
            className={`${button} bg-wine-500 text-white hover:bg-wine-600 dark:hover:bg-wine-600`}
          >
            {error ? <RotateCcw className="size-4" /> : <Download className="size-4" />}
            {error ? "Retry" : "Install"}
          </button>
        )}
      </div>
      {running && job && (
        <div className="flex flex-col gap-1">
          <ProgressBar percent={job.percent} />
          <div className={`flex justify-between ${subtle}`}>
            <span>{job.step}…</span>
            <span>
              {job.message}
              {job.percent !== null && ` · ${Math.floor(job.percent)}%`}
            </span>
          </div>
        </div>
      )}
      {error && <p className="text-xs text-wine-500 select-text dark:text-rose-400">{error}</p>}
    </li>
  );
}
