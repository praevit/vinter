import { useEffect, useState } from "react";
import { Download, FileUp, LoaderCircle } from "lucide-react";
import { commands, type RecipeSummary } from "../api/bindings";
import { bottleJobKey, useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs, useQuietJob } from "../api/jobs";
import { recipeBottles } from "../api/recipes";
import { useEnsureRosetta } from "../api/rosetta";
import { useInstalledRuntimes } from "../api/runtimes";
import { useSystemChecks } from "../api/system";
import { AppReady } from "./AppReady";
import { Dialog } from "./Dialog";
import { ProgressBar } from "./ProgressBar";
import type { Route } from "./Sidebar";
import { Button, ErrorText, Label, field, subtle } from "./ui";

const fileName = (path: string) => path.split(/[\\/]/).pop();

/**
 * Sets an app up from a recipe in a new bottle, and ends on the app.
 * `installer` is a file the user already has (e.g. one opened with Vinter);
 * it replaces the recipe's download.
 */
export function RecipeDialog({
  recipe,
  installer: given,
  onClose,
  onNavigate,
}: {
  recipe: RecipeSummary;
  installer?: string;
  onClose: () => void;
  onNavigate: (r: Route) => void;
}) {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const { data: status } = useSystemChecks();
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const [name, setName] = useState(recipe.name);
  const [installer, setInstaller] = useState<string | undefined>(given);
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState<string>();
  const [bottleId, setBottleId] = useState<string>();
  const job = bottleId ? jobs.job(bottleJobKey(bottleId)) : undefined;
  const running = isRunning(job);
  // This dialog shows a failed setup itself.
  useQuietJob(bottleId ? bottleJobKey(bottleId) : undefined);

  const needsInstaller = recipe.installer_kind === "user_provided";
  const needsConsent = recipe.winetricks_verbs.length > 0;
  const appleSilicon = status?.checks.some((c) => c.id === "rosetta" && c.status !== "skipped") ?? false;

  const failed = job?.done && !job.done.ok ? job.done.error : null;
  const bottle = bottles?.find((b) => b.id === bottleId);
  const needsRosetta = runtimes?.find((r) => r.id === bottle?.runtime)?.needs_rosetta ?? false;

  const choose = async () => {
    try {
      const picked = await commands.installerChoose();
      if (picked) setInstaller(picked);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const install = async () => {
    setError(undefined);
    if (!(await ensureRosetta(appleSilicon))) return;
    try {
      const started = await commands.recipeApply(recipe.id, name.trim() || recipe.name, installer ?? null, consent);
      recipeBottles.set(recipe.id, started.bottle_id);
      await jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      setBottleId(started.bottle_id);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const openBottle = () => {
    onClose();
    if (bottleId) onNavigate({ page: "bottle", id: bottleId });
  };

  if (job?.done?.ok && bottleId) {
    return (
      <Dialog onClose={onClose}>
        <AppReady bottleId={bottleId} needsRosetta={needsRosetta} onDone={onClose} />
      </Dialog>
    );
  }

  return (
    <Dialog onClose={onClose}>
      <div className="flex flex-col gap-4">
        <h2 className="text-lg font-semibold">Install {recipe.name}</h2>
        {failed ? (
          <div className="flex flex-col gap-3">
            <p className="text-sm">
              {bottle
                ? `Setting up ${recipe.name} did not finish. Open its bottle to try the rest again.`
                : `${recipe.name} could not be set up. Nothing was changed, so you can try again.`}
            </p>
            <details className="text-sm">
              <summary className="cursor-pointer text-frost-400">Details</summary>
              <p className="mt-2 text-xs break-words select-text">{failed}</p>
            </details>
            <div className="flex justify-end gap-2">
              <Button onClick={onClose}>Close</Button>
              {bottle ? (
                <Button variant="primary" onClick={openBottle}>
                  Open bottle
                </Button>
              ) : (
                <Button variant="primary" onClick={() => setBottleId(undefined)}>
                  Try again
                </Button>
              )}
            </div>
          </div>
        ) : running && job ? (
          <div className="flex flex-col gap-2 py-2">
            <p className="flex items-center gap-2 text-sm font-medium">
              <LoaderCircle className="size-4 animate-spin" /> {job.step}…
            </p>
            <ProgressBar percent={job.percent} />
            <p className={subtle}>{job.message}</p>
            <p className={subtle}>You can close this; the setup continues in the background.</p>
            <Button className="self-end" onClick={onClose}>
              Hide
            </Button>
          </div>
        ) : (
          <>
            <p className="text-sm text-slate-deep-700/80 dark:text-frost-300/80">{recipe.description}</p>
            {given ? (
              <p className="truncate text-sm" title={given}>
                Uses <strong>{fileName(given)}</strong> as the installer.
              </p>
            ) : (
              needsInstaller && (
                <InstallerPicker recipe={recipe} installer={installer} onChoose={choose} onFound={setInstaller} />
              )
            )}
            {needsConsent && (
              <label className="flex items-start gap-2 text-sm">
                <input
                  type="checkbox"
                  className="mt-1 accent-wine-500"
                  checked={consent}
                  onChange={(e) => setConsent(e.target.checked)}
                />
                <span>
                  This app also needs <strong>{recipe.winetricks_names.join(", ")}</strong>, which Vinter
                  downloads from Microsoft and others. I agree to their licenses.
                </span>
              </label>
            )}
            <details className="text-sm">
              <summary className="cursor-pointer text-frost-400">Options</summary>
              <div className="mt-2">
                <Label title="Bottle name">
                  <input className={field} value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
                </Label>
              </div>
            </details>
            {error && <ErrorText>{error}</ErrorText>}
            <div className="flex justify-end gap-2">
              <Button onClick={onClose}>Cancel</Button>
              <Button
                variant="primary"
                disabled={(needsInstaller && !installer) || (needsConsent && !consent)}
                onClick={install}
              >
                Install
              </Button>
            </div>
          </>
        )}
      </div>
    </Dialog>
  );
}

/**
 * For recipes whose installer the user downloads: a link to the download
 * page, then a watch on the Downloads folder for the file.
 */
function InstallerPicker({
  recipe,
  installer,
  onChoose,
  onFound,
}: {
  recipe: RecipeSummary;
  installer: string | undefined;
  onChoose: () => void;
  onFound: (path: string) => void;
}) {
  // When the download page was opened (ms since 1970); Downloads is watched from then.
  const [since, setSince] = useState<number>();
  const [found, setFound] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (since === undefined || installer) return;
    let stop = false;
    const look = async () => {
      try {
        const files = await commands.recipeFindDownloads(recipe.id, since);
        if (!stop && files.length) setFound(files[0]);
      } catch {
        // Downloads may not be readable; choosing the file still works.
      }
    };
    void look();
    const timer = setInterval(look, 2000);
    return () => {
      stop = true;
      clearInterval(timer);
    };
  }, [since, installer, recipe.id]);

  const openPage = async () => {
    setError(undefined);
    try {
      await commands.recipeOpenDownloadPage(recipe.id);
      setSince((s) => s ?? Date.now());
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm">{recipe.installer_hint}</p>
      <div className="flex flex-wrap gap-2">
        {recipe.download_page && (
          <Button onClick={openPage}>
            <Download className="size-4" /> Open download page
          </Button>
        )}
        <Button onClick={onChoose}>
          <FileUp className="size-4" />
          {installer ? "Choose another…" : "Choose installer…"}
        </Button>
      </div>
      {installer ? (
        <p className="truncate text-sm font-medium" title={installer}>
          {fileName(installer)}
        </p>
      ) : found ? (
        <div className="flex items-center gap-2 rounded-lg bg-frost-100 px-3 py-2 text-sm dark:bg-slate-deep-800">
          <span className="min-w-0 flex-1 truncate" title={found}>
            Found {fileName(found)}. Use it?
          </span>
          <Button variant="primary" onClick={() => onFound(found)}>
            Use it
          </Button>
        </div>
      ) : (
        since !== undefined && (
          <p className={`flex items-center gap-2 ${subtle}`}>
            <LoaderCircle className="size-3 animate-spin" /> Waiting for the download in your Downloads folder…
          </p>
        )
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
