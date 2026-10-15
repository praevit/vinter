import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands, type UpdateInfo } from "../api/bindings";
import { useAppInfo } from "../api/app";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { ProgressBar } from "../components/ProgressBar";
import { Button, subtle } from "../components/ui";

const UPDATE_JOB = "app-update";

/** The small "Software Update" window opened from the menu bar. */
export function UpdatesWindow() {
  const { data: info } = useAppInfo();
  const jobs = useJobs();
  const job = jobs.job(UPDATE_JOB);
  // undefined while checking; null when up to date.
  const [update, setUpdate] = useState<UpdateInfo | null>();
  const [error, setError] = useState<string>();
  // An install failure only counts until the next check.
  const [installStarted, setInstallStarted] = useState(false);

  const check = async () => {
    setUpdate(undefined);
    setError(undefined);
    setInstallStarted(false);
    try {
      setUpdate(await commands.updateCheck());
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  useEffect(() => {
    void check();
  }, []);

  const close = () => getCurrentWindow().close();
  const installing = isRunning(job);
  const installFailed = installStarted && job?.done && !job.done.ok;
  const failed = error ?? (installFailed ? (job.done?.error ?? "The update failed.") : undefined);
  const install = () => {
    setInstallStarted(true);
    void jobs.start(UPDATE_JOB, () => commands.updateInstall());
  };

  let title: string;
  let detail: string;
  if (failed) {
    title = "Could not update Vinter";
    detail = failed;
  } else if (installing) {
    title = `Updating to Vinter ${update?.version ?? ""}`;
    detail = `${job?.step ?? "Starting"}… Vinter restarts when it is done.`;
  } else if (update === undefined) {
    title = "Checking for updates…";
    detail = "";
  } else if (update === null) {
    title = "Vinter is up to date";
    detail = `Version ${info?.version ?? ""} is the latest version.`;
  } else {
    title = "A new version of Vinter is available";
    detail = `Vinter ${update.version} is available. You have ${info?.version ?? "an older version"}.`;
  }

  return (
    <div className="flex h-screen flex-col gap-4 p-5 select-none">
      <div className="flex gap-4">
        <img src="/vinter.svg" alt="" className="size-14 shrink-0" draggable={false} />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <h1 className="text-sm font-semibold">{title}</h1>
          {(update === undefined && !failed) || installing ? (
            <ProgressBar percent={installing ? (job?.percent ?? null) : null} />
          ) : null}
          {detail && <p className={`${subtle} line-clamp-3 select-text`}>{detail}</p>}
        </div>
      </div>
      <div className="mt-auto flex justify-end gap-2">
        {failed ? (
          <>
            <Button onClick={close}>Close</Button>
            <Button variant="primary" onClick={check}>
              Try Again
            </Button>
          </>
        ) : update && !installing ? (
          <>
            <Button onClick={close}>Later</Button>
            <Button variant="primary" onClick={install}>
              Install and Restart
            </Button>
          </>
        ) : update === null ? (
          <Button variant="primary" onClick={close}>
            OK
          </Button>
        ) : (
          <Button onClick={close} disabled={installing}>
            Cancel
          </Button>
        )}
      </div>
    </div>
  );
}
