import { useState, type ReactNode } from "react";
import { ExternalLink, FolderOpen, Heart, LoaderCircle, RefreshCw } from "lucide-react";
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

  const updateStatus = isRunning(job) ? (
    <div className="flex w-56 flex-col gap-1">
      <ProgressBar percent={job?.percent ?? null} />
      <p className={subtle}>{job?.step}… Vinter restarts when it is done.</p>
    </div>
  ) : update ? (
    <Button variant="primary" onClick={() => jobs.start(UPDATE_JOB, () => commands.updateInstall())}>
      Install {update.version} and restart
    </Button>
  ) : (
    <Button onClick={check} disabled={checking}>
      {checking ? <LoaderCircle className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
      Check for updates
    </Button>
  );

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">
      <header className="flex flex-wrap items-center gap-5 rounded-2xl border border-frost-200 bg-white p-6 dark:border-slate-deep-700 dark:bg-slate-deep-900">
        <img src="/vinter.svg" alt="" className="size-16 shrink-0" draggable={false} />
        <div className="flex min-w-0 flex-1 flex-col">
          <h1 className="text-2xl font-semibold tracking-tight">Vinter</h1>
          <p className="text-sm text-slate-deep-700/70 dark:text-frost-300/70">Windows apps, poured onto your Mac.</p>
          <p className={`${subtle} mt-1`}>
            Version {info?.version}
            {update === null && " · You have the latest version."}
            {update && ` · Vinter ${update.version} is available.`}
          </p>
        </div>
        {updateStatus}
      </header>
      {(error || (job?.done && !job.done.ok)) && <ErrorText>{error ?? job?.done?.error}</ErrorText>}
      {update?.notes && !isRunning(job) && (
        <p className={`-mt-3 whitespace-pre-wrap px-1 ${subtle}`}>{update.notes}</p>
      )}

      <Group>
        <Row title="Support Vinter" detail="Vinter is free. If it saves you a Windows license or a virtual machine, you can sponsor its development on GitHub.">
          <Button variant="primary" onClick={() => open("support")}>
            <Heart className="size-4" /> Sponsor
          </Button>
        </Row>
        <Row title="Website" detail="getvinter.app">
          <Button onClick={() => open("website")}>
            <ExternalLink className="size-4" /> Open
          </Button>
        </Row>
        <Row title="Source code" detail="Free software under the GNU GPL, version 3 or later.">
          <Button onClick={() => open("source")}>
            <ExternalLink className="size-4" /> GitHub
          </Button>
        </Row>
        <Row title="Report a problem" detail="Include the debug report from the bottle's Logs tab.">
          <Button onClick={() => open("issues")}>
            <ExternalLink className="size-4" /> Open an issue
          </Button>
        </Row>
      </Group>

      <Group>
        <Row title="Data folder" detail={info?.data_dir} selectable>
          <Button onClick={() => open("data_folder")}>
            <FolderOpen className="size-4" /> Show
          </Button>
        </Row>
        <Row title="Launchers folder" detail={info?.launchers_dir} selectable>
          <Button onClick={() => open("launchers_folder")}>
            <FolderOpen className="size-4" /> Show
          </Button>
        </Row>
      </Group>

      <footer className={`flex flex-col gap-1.5 px-1 ${subtle}`}>
        <p>
          Vinter runs programs with{" "}
          <button type="button" className="underline" onClick={() => open("wine_license")}>
            Wine
          </button>
          , licensed under the LGPL. Each runtime folder includes Wine's license and a link to its exact
          source. Graphics: DXVK (zlib license), MoltenVK (Apache 2.0). Winetricks (LGPL) for some recipes.
        </p>
        <p>
          "Vinter" and its logo are trademarks of Prævit ehf. Vinter is not affiliated with Wine, Apple or
          Microsoft. Made in Iceland by Prævit ehf.
        </p>
      </footer>
    </div>
  );
}

/** A rounded box of rows with dividers, like a settings list. */
function Group({ children }: { children: ReactNode }) {
  return (
    <section className="divide-y divide-frost-200 rounded-2xl border border-frost-200 bg-white dark:divide-slate-deep-700 dark:border-slate-deep-700 dark:bg-slate-deep-900">
      {children}
    </section>
  );
}

function Row({
  title,
  detail,
  selectable,
  children,
}: {
  title: string;
  detail?: string;
  selectable?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 px-5 py-3.5">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-sm font-medium">{title}</span>
        {detail && <span className={`${subtle} ${selectable ? "truncate select-text" : ""}`} title={detail}>{detail}</span>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
