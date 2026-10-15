import { useEffect, useMemo, useState } from "react";
import { FileUp, FolderOpen, LoaderCircle, PackagePlus, Search, TriangleAlert } from "lucide-react";
import { commands, type RecipeSummary } from "../api/bindings";
import { bottleJobKey, useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { recipeBottles, useRecipes } from "../api/recipes";
import { useEnsureRosetta } from "../api/rosetta";
import { formatBytes } from "../api/runtimes";
import { useSystemChecks } from "../api/system";
import { Dialog } from "../components/Dialog";
import { ProgressBar } from "../components/ProgressBar";
import type { Route } from "../components/Sidebar";
import { Button, ErrorText, Label, field, subtle } from "../components/ui";

export function InstallApp({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const { data } = useRecipes();
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<RecipeSummary>();

  const recipes = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data?.recipes ?? []).filter(
      (r) => !q || r.name.toLowerCase().includes(q) || r.description.toLowerCase().includes(q),
    );
  }, [data, query]);

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Install an App</h1>
        <p className="mt-1 text-sm text-slate-deep-700/70 dark:text-frost-300/70">
          Recipes set up a fresh bottle with everything an app needs. Pick one, or use your own installer.
        </p>
      </header>

      <label className="relative">
        <Search className="pointer-events-none absolute top-2 left-2.5 size-4 text-frost-400" />
        <input
          className={`${field} pl-8`}
          placeholder="Search apps"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search apps"
        />
      </label>

      <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        {recipes.map((r) => (
          <RecipeCard key={r.id} recipe={r} onOpen={() => setOpen(r)} onNavigate={onNavigate} />
        ))}
        <li>
          <button
            type="button"
            onClick={() => onNavigate({ page: "new-bottle" })}
            className="flex h-full w-full flex-col items-start gap-1 rounded-xl border border-dashed border-frost-300 p-4 text-left hover:border-wine-500/60 dark:border-slate-deep-700"
          >
            <span className="flex items-center gap-2 font-medium">
              <FileUp className="size-4" /> Custom installer
            </span>
            <span className={subtle}>
              Something else? Create an empty bottle, then install any Windows installer into it.
            </span>
          </button>
        </li>
      </ul>
      {recipes.length === 0 && query && <p className={subtle}>No recipe matches "{query}".</p>}

      {(data?.broken.length ?? 0) > 0 && (
        <section className="flex flex-col gap-2 rounded-xl border border-amber-300/60 bg-amber-50 p-4 text-sm dark:border-amber-500/30 dark:bg-amber-500/10">
          <p className="flex items-center gap-2 font-medium">
            <TriangleAlert className="size-4 text-amber-600" /> Some of your recipes could not be loaded
          </p>
          {data?.broken.map((b) => (
            <div key={b.file}>
              <p className="font-mono text-xs select-text">{b.file}</p>
              <p className="text-xs select-text">{b.error}</p>
            </div>
          ))}
        </section>
      )}

      <div className="flex items-center gap-3">
        <Button onClick={() => commands.recipesOpenFolder()}>
          <FolderOpen className="size-4" /> Your recipes folder
        </Button>
        <span className={subtle}>Add your own .toml recipes there. They appear here and can replace built-in ones.</span>
      </div>

      {open && <RecipeDialog recipe={open} onClose={() => setOpen(undefined)} onNavigate={onNavigate} />}
    </div>
  );
}

function RecipeCard({
  recipe,
  onOpen,
  onNavigate,
}: {
  recipe: RecipeSummary;
  onOpen: () => void;
  onNavigate: (r: Route) => void;
}) {
  const jobs = useJobs();
  const { data: bottles } = useBottles();
  const bottleId = recipeBottles.get(recipe.id);
  const job = bottleId ? jobs.job(bottleJobKey(bottleId)) : undefined;
  const running = isRunning(job);
  const bottleExists = !!bottleId && !!bottles?.some((b) => b.id === bottleId);

  return (
    <li className="flex flex-col gap-2 rounded-xl border border-frost-200 bg-white p-4 dark:border-slate-deep-700 dark:bg-slate-deep-900">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h2 className="font-medium">{recipe.name}</h2>
          <p className={subtle}>{recipe.description}</p>
        </div>
        {!recipe.builtin && (
          <span className="shrink-0 rounded bg-frost-100 px-1.5 py-0.5 text-[11px] dark:bg-slate-deep-800">Yours</span>
        )}
      </div>
      <p className={subtle}>
        {recipe.installer_kind === "download"
          ? `Installer downloaded for you${recipe.download_size ? ` (${formatBytes(recipe.download_size)})` : ""}`
          : recipe.installer_kind === "user_provided"
            ? "You provide the installer"
            : "No installer needed"}
        {recipe.license ? ` · ${recipe.license}` : ""}
      </p>
      {running && job ? (
        <div className="flex flex-col gap-1">
          <ProgressBar percent={job.percent} />
          <p className={subtle}>
            {job.step}… {job.message}
          </p>
        </div>
      ) : job?.done && bottleExists ? (
        <div className="flex items-center gap-2">
          <span className={subtle}>{job.done.ok ? "Installed." : "Setup stopped."}</span>
          <Button onClick={() => onNavigate({ page: "bottle", id: bottleId })}>Open bottle</Button>
        </div>
      ) : (
        <Button variant="primary" className="self-start" onClick={onOpen}>
          <PackagePlus className="size-4" /> Install
        </Button>
      )}
    </li>
  );
}

function RecipeDialog({
  recipe,
  onClose,
  onNavigate,
}: {
  recipe: RecipeSummary;
  onClose: () => void;
  onNavigate: (r: Route) => void;
}) {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const { data: status } = useSystemChecks();
  const { data: bottles } = useBottles();
  const [name, setName] = useState(recipe.name);
  const [installer, setInstaller] = useState<string>();
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState<string>();
  const [bottleId, setBottleId] = useState<string>();
  const job = bottleId ? jobs.job(bottleJobKey(bottleId)) : undefined;
  const running = isRunning(job);

  const needsInstaller = recipe.installer_kind === "user_provided";
  const needsConsent = recipe.winetricks_verbs.length > 0;
  const appleSilicon = status?.checks.some((c) => c.id === "rosetta" && c.status !== "skipped") ?? false;

  const failed = job?.done && !job.done.ok ? job.done.error : null;
  const bottleExists = !!bottleId && !!bottles?.some((b) => b.id === bottleId);

  // When it succeeds, go to the new bottle.
  useEffect(() => {
    if (job?.done?.ok && bottleId) {
      onClose();
      onNavigate({ page: "bottle", id: bottleId });
    }
  }, [job?.done?.ok, bottleId, onClose, onNavigate]);

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
      const started = await commands.recipeApply(recipe.id, name, installer ?? null, consent);
      recipeBottles.set(recipe.id, started.bottle_id);
      await jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      setBottleId(started.bottle_id);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Dialog onClose={running ? undefined : onClose}>
      <div className="flex flex-col gap-4">
        <h2 className="text-lg font-semibold">Install {recipe.name}</h2>
        {failed ? (
          <div className="flex flex-col gap-3">
            <ErrorText>{failed}</ErrorText>
            <p className={subtle}>
              {bottleExists
                ? "The bottle was created. Open it to see what happened and resume the setup."
                : "Nothing was created. You can try again."}
            </p>
            <div className="flex justify-end gap-2">
              <Button onClick={onClose}>Close</Button>
              {bottleExists && bottleId && (
                <Button
                  variant="primary"
                  onClick={() => {
                    onClose();
                    onNavigate({ page: "bottle", id: bottleId });
                  }}
                >
                  Open bottle
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
            <Label title="Bottle name">
              <input className={field} value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
            </Label>
            {needsInstaller && (
              <div className="flex flex-col gap-2">
                <p className="text-sm">{recipe.installer_hint}</p>
                <Button onClick={choose} className="self-start">
                  <FileUp className="size-4" />
                  {installer ? "Choose another…" : "Choose installer…"}
                </Button>
                {installer && (
                  <p className="truncate text-sm font-medium" title={installer}>
                    {installer.split(/[\\/]/).pop()}
                  </p>
                )}
              </div>
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
                  This recipe uses winetricks to install <strong>{recipe.winetricks_verbs.join(", ")}</strong>,
                  which may download software from Microsoft and others. I agree to their licenses.
                </span>
              </label>
            )}
            {error && <ErrorText>{error}</ErrorText>}
            <div className="flex justify-end gap-2">
              <Button onClick={onClose}>Cancel</Button>
              <Button
                variant="primary"
                disabled={!name.trim() || (needsInstaller && !installer) || (needsConsent && !consent)}
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
