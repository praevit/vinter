import { useState } from "react";
import { ExternalLink, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { commands, type AppLink, type UpdateInfo } from "../api/bindings";
import { useAppInfo } from "../api/app";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { ProgressBar } from "../components/ProgressBar";
import { Button, ErrorText, subtle } from "../components/ui";

const UPDATE_JOB = "app-update";

export function About() {
  const { data: info } = useAppInfo();
  const jobs = useJobs();
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>();
  const [error, setError] = useState<string>();
  const job = jobs.job(UPDATE_JOB);

  const open = (link: AppLink) => commands.appOpenLink(link).catch((e) => setError(errorMessage(e)));

  const check = async () => {
    setChecking(true);
    setError(undefined);
    try {
      setUpdate(await commands.updateCheck());
    } catch (e) {
      setError(`Could not check for updates: ${errorMessage(e)}`);
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-8 px-8 py-10">
      <header className="flex items-center gap-4">
        <img src="/vinter.svg" alt="" className="size-20" draggable={false} />
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">Vinter</h1>
          <p className="text-slate-deep-700/70 dark:text-frost-300/70">Windows apps, poured onto your Mac.</p>
          <p className={subtle}>Version {info?.version}</p>
        </div>
      </header>

      <section className="flex flex-col items-start gap-2">
        <h2 className="text-sm font-medium">Updates</h2>
        {isRunning(job) ? (
          <div className="flex w-full flex-col gap-1">
            <ProgressBar percent={job?.percent ?? null} />
            <p className={subtle}>{job?.step}… Vinter restarts when it is done.</p>
          </div>
        ) : update ? (
          <>
            <p className="text-sm">Vinter {update.version} is available.</p>
            {update.notes && <p className={`whitespace-pre-wrap ${subtle}`}>{update.notes}</p>}
            <Button variant="primary" onClick={() => jobs.start(UPDATE_JOB, () => commands.updateInstall())}>
              Install and restart
            </Button>
          </>
        ) : (
          <>
            {update === null && <p className="text-sm">You have the latest version.</p>}
            <Button onClick={check} disabled={checking}>
              {checking ? <LoaderCircle className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
              Check for updates
            </Button>
          </>
        )}
        {(error || (job?.done && !job.done.ok)) && <ErrorText>{error ?? job?.done?.error}</ErrorText>}
      </section>

      <section className="flex flex-col items-start gap-2">
        <h2 className="text-sm font-medium">Folders</h2>
        <p className={`select-text ${subtle}`}>Data: {info?.data_dir}</p>
        <p className={`select-text ${subtle}`}>Launchers: {info?.launchers_dir}</p>
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => open("data_folder")}>
            <FolderOpen className="size-4" /> Data folder
          </Button>
          <Button onClick={() => open("launchers_folder")}>
            <FolderOpen className="size-4" /> Launchers folder
          </Button>
        </div>
      </section>

      <section className="flex flex-col items-start gap-2">
        <h2 className="text-sm font-medium">Links</h2>
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => open("website")}>
            <ExternalLink className="size-4" /> getvinter.app
          </Button>
          <Button onClick={() => open("source")}>
            <ExternalLink className="size-4" /> Source code
          </Button>
          <Button onClick={() => open("issues")}>
            <ExternalLink className="size-4" /> Report a problem
          </Button>
        </div>
      </section>

      <section className="flex flex-col gap-1 text-sm text-slate-deep-700/80 dark:text-frost-300/80">
        <h2 className="text-sm font-medium text-current">Credits</h2>
        <p>
          Vinter runs programs with{" "}
          <button type="button" className="underline" onClick={() => open("wine_license")}>
            Wine
          </button>
          , licensed under the LGPL. Each runtime folder includes Wine's license and a link to its exact source.
        </p>
        <p>Graphics: DXVK (zlib license), MoltenVK (Apache 2.0). Winetricks (LGPL) for some recipes.</p>
        <p>Vinter is not affiliated with Wine, Apple or Microsoft.</p>
        <p className={subtle}>Made in Iceland by Prævit ehf.</p>
      </section>
    </div>
  );
}
