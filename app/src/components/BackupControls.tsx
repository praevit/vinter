import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ArchiveRestore, HardDriveDownload } from "lucide-react";
import { commands } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { ProgressBar } from "./ProgressBar";
import { Button, ErrorText, subtle } from "./ui";

export const RESTORE_JOB = "bottle-restore";

/** Progress and result of a backup or restore job. */
function JobStatus({ jobKey, done }: { jobKey: string; done: string }) {
  const job = useJobs().job(jobKey);
  if (!job) return null;
  if (isRunning(job))
    return (
      <div className="flex w-full flex-col gap-1">
        <ProgressBar percent={job.percent ?? null} />
        <p className={subtle}>
          {job.step}
          {job.message && ` · ${job.message}`}
        </p>
      </div>
    );
  return job.done?.ok ? <p className={subtle}>{done}</p> : <ErrorText>{job.done?.error}</ErrorText>;
}

/** The job key of a bottle's backup, for its progress. */
export const backupJobKey = (bottleId: string) => `bottle-backup:${bottleId}`;

/** Asks where to save a backup of the bottle, then starts it. Resolves to an error message, if any. */
export async function startBackup(
  jobs: ReturnType<typeof useJobs>,
  bottleId: string,
): Promise<string | undefined> {
  try {
    const jobId = await commands.bottleExport(bottleId);
    if (jobId) await jobs.start(backupJobKey(bottleId), async () => jobId);
  } catch (e) {
    return errorMessage(e);
  }
}

/** "Back up…" for a bottle's Snapshots & backups tab. */
export function BackupButton({ bottleId }: { bottleId: string }) {
  const jobs = useJobs();
  const key = backupJobKey(bottleId);
  const [error, setError] = useState<string>();

  const start = async () => {
    setError(undefined);
    setError(await startBackup(jobs, bottleId));
  };

  return (
    <section className="flex flex-col items-start gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">Back up</h2>
      <p className={subtle}>
        Saves the whole bottle, with its programs and settings, as one .vinterbottle file. Restore it
        later, or on another computer, from New bottle. Close its programs first.
      </p>
      <Button onClick={start} disabled={isRunning(jobs.job(key))}>
        <HardDriveDownload className="size-4" /> Back up…
      </Button>
      <JobStatus jobKey={key} done="Backup saved." />
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}

/** "Restore from a backup…", optionally for a file already chosen. */
export function RestoreButton({ path }: { path?: string }) {
  const jobs = useJobs();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string>();

  const start = async () => {
    setError(undefined);
    try {
      const jobId = await commands.bottleImport(path ?? null);
      if (jobId) {
        await jobs.start(RESTORE_JOB, async () => jobId);
        await queryClient.invalidateQueries({ queryKey: ["bottles"] });
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-col items-start gap-2">
      <Button onClick={start} disabled={isRunning(jobs.job(RESTORE_JOB))}>
        <ArchiveRestore className="size-4" /> {path ? "Restore" : "Restore from a backup…"}
      </Button>
      <JobStatus jobKey={RESTORE_JOB} done="Restored. The bottle is in the sidebar." />
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
