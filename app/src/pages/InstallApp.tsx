import { useMemo, useState } from "react";
import { FileUp, FolderOpen, PackagePlus, RotateCcw, Search, TriangleAlert } from "lucide-react";
import { commands, type RecipeSummary } from "../api/bindings";
import { bottleJobKey, useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { recipeBottles, useRecipes } from "../api/recipes";
import { formatBytes } from "../api/runtimes";
import { OpenFileDialog } from "../components/OpenFileDialog";
import { ProgressBar } from "../components/ProgressBar";
import { RecipeDialog } from "../components/RecipeDialog";
import type { Route } from "../components/Sidebar";
import { Button, ErrorText, field, subtle } from "../components/ui";

export function InstallApp({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const { data, isPending, isError, error, refetch, isFetching } = useRecipes();
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<RecipeSummary>();
  // An installer the user picked for "Custom installer".
  const [custom, setCustom] = useState<string>();
  const [chooseError, setChooseError] = useState<string>();

  const recipes = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data?.recipes ?? []).filter(
      (r) => !q || r.name.toLowerCase().includes(q) || r.description.toLowerCase().includes(q),
    );
  }, [data, query]);

  const chooseInstaller = async () => {
    setChooseError(undefined);
    try {
      const picked = await commands.installerChoose();
      if (picked) setCustom(picked);
    } catch (e) {
      setChooseError(errorMessage(e));
    }
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Install an App</h1>
        <p className="mt-1 text-sm text-slate-deep-700/70 dark:text-frost-300/70">
          Pick an app and Vinter sets up everything it needs. Not listed? Use its own installer.
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

      {isError ? (
        <div className="flex flex-col items-start gap-2 rounded-xl border border-frost-200 p-4 dark:border-slate-deep-700">
          <p className="text-sm font-medium">The list of apps could not be loaded.</p>
          <p className={`${subtle} select-text`}>{errorMessage(error)}</p>
          <Button onClick={() => void refetch()} disabled={isFetching}>
            <RotateCcw className="size-4" /> Retry
          </Button>
        </div>
      ) : (
        <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {isPending
            ? [0, 1, 2, 3].map((i) => (
                <li
                  key={i}
                  aria-hidden
                  className="flex h-36 animate-pulse flex-col gap-2 rounded-xl border border-frost-200 bg-white p-4 dark:border-slate-deep-700 dark:bg-slate-deep-900"
                >
                  <span className="h-4 w-1/3 rounded bg-frost-200 dark:bg-slate-deep-700" />
                  <span className="h-3 w-5/6 rounded bg-frost-100 dark:bg-slate-deep-800" />
                  <span className="h-3 w-1/2 rounded bg-frost-100 dark:bg-slate-deep-800" />
                  <span className="mt-auto h-7 w-20 rounded bg-frost-200 dark:bg-slate-deep-700" />
                </li>
              ))
            : recipes.map((r) => (
                <RecipeCard key={r.id} recipe={r} onOpen={() => setOpen(r)} onNavigate={onNavigate} />
              ))}
          <li>
            <div className="flex h-full w-full flex-col items-start gap-1 rounded-xl border border-dashed border-frost-300 p-4 text-left dark:border-slate-deep-700">
              <div className="flex flex-col items-start gap-1">
                <span className="flex items-center gap-2 font-medium">
                  <FileUp className="size-4" /> Custom installer
                </span>
                <span className={subtle}>
                  Something else? Choose its Windows installer, and Vinter sets up a bottle for it.
                </span>
              </div>
              <Button className="mt-2" onClick={chooseInstaller}>
                Choose installer…
              </Button>
              <button
                type="button"
                onClick={() => onNavigate({ page: "new-bottle" })}
                className={`${subtle} mt-1 underline-offset-2 hover:underline`}
              >
                Or create an empty bottle
              </button>
            </div>
          </li>
        </ul>
      )}
      {chooseError && <ErrorText>{chooseError}</ErrorText>}
      {!isPending && recipes.length === 0 && query && (
        <div className="flex flex-col items-start gap-2">
          <p className="text-sm">No app called "{query}" here. Not listed? Install it from its own installer.</p>
          <Button variant="primary" onClick={chooseInstaller}>
            <FileUp className="size-4" /> Choose installer…
          </Button>
        </div>
      )}

      <section className="mt-6 flex flex-col gap-3 border-t border-frost-200 pt-4 dark:border-slate-deep-700">
        <h2 className={`${subtle} font-medium tracking-wide uppercase`}>For recipe authors</h2>
        {(data?.broken.length ?? 0) > 0 && (
          <div className="flex flex-col gap-2 text-xs text-slate-deep-700/70 dark:text-frost-300/70">
            <p className="flex items-center gap-2 font-medium">
              <TriangleAlert className="size-3.5 text-amber-600" /> Some of your recipes could not be loaded
            </p>
            {data?.broken.map((b) => (
              <div key={b.file}>
                <p className="font-mono select-text">{b.file}</p>
                <p className="select-text">{b.error}</p>
              </div>
            ))}
          </div>
        )}
        <div className="flex items-center gap-3">
          <Button onClick={() => commands.recipesOpenFolder()}>
            <FolderOpen className="size-4" /> Your recipes folder
          </Button>
          <span className={subtle}>Add your own .toml recipes there. They appear here and can replace built-in ones.</span>
        </div>
      </section>

      {open && <RecipeDialog recipe={open} onClose={() => setOpen(undefined)} onNavigate={onNavigate} />}
      {custom && <OpenFileDialog key={custom} path={custom} onNavigate={onNavigate} onClose={() => setCustom(undefined)} />}
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
