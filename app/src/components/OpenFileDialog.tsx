import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileUp, LoaderCircle, Plus, Sparkles } from "lucide-react";
import { commands } from "../api/bindings";
import { bottleJobKey, useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs, useQuietJob } from "../api/jobs";
import { useEnsureRosetta } from "../api/rosetta";
import { useDefaultRuntime, useInstalledRuntimes } from "../api/runtimes";
import { useSystemChecks } from "../api/system";
import { Dialog } from "./Dialog";
import { RestoreButton } from "./BackupControls";
import { InstallDialog } from "./InstallDialog";
import { ProgressBar } from "./ProgressBar";
import { RecipeDialog } from "./RecipeDialog";
import { Select } from "./Select";
import type { Route } from "./Sidebar";
import { Button, ErrorText, Label, subtle } from "./ui";

/**
 * A .exe or .msi opened with Vinter (Finder, Open With, drag and drop):
 * offers the tested recipe if the file is a known app's installer, else asks
 * which bottle to run it in (or sets one up), then continues like "Install
 * a program".
 */
export function OpenFileDialog({
  path,
  onNavigate,
  onClose,
}: {
  path: string;
  onNavigate: (r: Route) => void;
  onClose: () => void;
}) {
  const isBackup = /\.vinterbottle$/i.test(path);
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const { data: defaultRuntime } = useDefaultRuntime();
  const { data: status } = useSystemChecks();
  const { data: recipe } = useQuery({
    queryKey: ["recipe-for-installer", path],
    queryFn: () => commands.recipeForInstaller(path),
    enabled: !isBackup,
  });
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const ready = bottles?.filter((b) => b.status === "ready") ?? [];
  const [bottleId, setBottleId] = useState<string>();
  const [confirmed, setConfirmed] = useState(false);
  const [useRecipe, setUseRecipe] = useState(false);
  // A bottle being set up for this file.
  const [newBottle, setNewBottle] = useState<string>();
  const [error, setError] = useState<string>();
  const setupJob = newBottle ? jobs.job(bottleJobKey(newBottle)) : undefined;
  // This dialog shows a failed setup itself.
  useQuietJob(newBottle ? bottleJobKey(newBottle) : undefined);
  const chosen = ready.find((b) => b.id === (newBottle ?? bottleId)) ?? (newBottle ? undefined : ready[0]);
  const fileName = path.split(/[\\/]/).pop();
  const appleSilicon = status?.checks.some((c) => c.id === "rosetta" && c.status !== "skipped") ?? false;

  if (isBackup) {
    return (
      <Dialog onClose={onClose}>
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Restore {fileName}?</h2>
          <p className={subtle}>The backup becomes a new bottle. Your other bottles are not changed.</p>
          <RestoreButton path={path} />
          <div className="flex justify-end">
            <Button onClick={onClose}>Close</Button>
          </div>
        </div>
      </Dialog>
    );
  }

  if (useRecipe && recipe) {
    return <RecipeDialog recipe={recipe} installer={path} onClose={onClose} onNavigate={onNavigate} />;
  }

  // A new bottle that is ready, or a chosen one: run the installer in it.
  if ((confirmed || setupJob?.done?.ok) && chosen) {
    return (
      <InstallDialog
        bottleId={chosen.id}
        needsRosetta={runtimes?.find((r) => r.id === chosen.runtime)?.needs_rosetta ?? false}
        start="choose"
        initialPath={path}
        onClose={() => {
          onNavigate({ page: "bottle", id: chosen.id });
          onClose();
        }}
      />
    );
  }

  const setUp = async () => {
    setError(undefined);
    // With no runtime installed, the one recipes use is installed (Intel, so Rosetta).
    const runtime = runtimes?.find((r) => r.id === defaultRuntime);
    if (!(await ensureRosetta(runtime ? runtime.needs_rosetta : appleSilicon))) return;
    try {
      const started = await commands.installerSetupBottle(path);
      await jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      setNewBottle(started.bottle_id);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  if (newBottle && setupJob) {
    const failed = setupJob.done && !setupJob.done.ok ? setupJob.done.error : null;
    return (
      <Dialog onClose={onClose}>
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Setting up a bottle for {fileName}</h2>
          {failed ? (
            <>
              <p className="text-sm">The bottle could not be set up.</p>
              <details className="text-sm">
                <summary className="cursor-pointer text-frost-400">Details</summary>
                <p className="mt-2 text-xs break-words select-text">{failed}</p>
              </details>
              <div className="flex justify-end gap-2">
                <Button onClick={onClose}>Close</Button>
                <Button variant="primary" onClick={() => setNewBottle(undefined)}>
                  Try again
                </Button>
              </div>
            </>
          ) : (
            <>
              <p className="flex items-center gap-2 text-sm font-medium">
                <LoaderCircle className="size-4 animate-spin" /> {setupJob.step || "Starting"}…
              </p>
              <ProgressBar percent={setupJob.percent} />
              {isRunning(setupJob) && <p className={subtle}>{setupJob.message}</p>}
              <p className={subtle}>When it is ready, the installer starts.</p>
            </>
          )}
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog onClose={onClose}>
      <div className="flex flex-col gap-4">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <FileUp className="size-5" /> Open {fileName}
        </h2>
        {recipe && (
          <button
            type="button"
            onClick={() => setUseRecipe(true)}
            className="flex flex-col items-start gap-1 rounded-xl border border-wine-500/50 p-3 text-left hover:bg-frost-100 dark:hover:bg-slate-deep-800"
          >
            <span className="flex items-center gap-2 text-sm font-medium">
              <Sparkles className="size-4 text-wine-500" /> Use the tested setup for {recipe.name} (recommended)
            </span>
            <span className={subtle}>Vinter sets up a bottle with everything {recipe.name} needs, then runs this installer.</span>
          </button>
        )}
        {ready.length === 0 ? (
          <p className={subtle}>
            {recipe ? "Or run it on its own: " : ""}Windows programs run inside a bottle. Vinter can set one
            up for this installer; nothing is installed on your Mac itself.
          </p>
        ) : (
          <>
            <p className={subtle}>
              {recipe ? "Or choose" : "Choose"} the bottle to run it in. An installer puts its program in that
              bottle; nothing is installed on your Mac itself.
            </p>
            <Label title="Bottle">
              <Select
                value={chosen?.id ?? ""}
                onChange={setBottleId}
                options={ready.map((b) => ({ value: b.id, label: b.name }))}
              />
            </Label>
          </>
        )}
        {error && <ErrorText>{error}</ErrorText>}
        <div className="flex flex-wrap justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          {ready.length === 0 ? (
            <Button variant={recipe ? "plain" : "primary"} onClick={setUp}>
              <Plus className="size-4" /> Set up a bottle for {fileName}
            </Button>
          ) : (
            <>
              <Button onClick={setUp}>
                <Plus className="size-4" /> New bottle
              </Button>
              <Button variant={recipe ? "plain" : "primary"} onClick={() => setConfirmed(true)}>
                Continue
              </Button>
            </>
          )}
        </div>
      </div>
    </Dialog>
  );
}
