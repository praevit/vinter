import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Copy, Eraser, FolderOpen, Square, Trash2 } from "lucide-react";
import { commands, type Tool } from "../api/bindings";
import {
  TOOLS,
  WINDOWS_VERSIONS,
  bottleJobKey,
  useBottle,
  useBottles,
  useDeleteBottle,
  useUpdateBottle,
  optionsJobKey,
  windowsJobKey,
  windowsLabel,
  type BottleDetails,
} from "../api/bottles";
import { useAppInfo } from "../api/app";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { useEnsureRosetta } from "../api/rosetta";
import { useRecipeState } from "../api/recipes";
import { formatBytes, useInstalledRuntimes } from "../api/runtimes";
import { RecipeResume } from "../components/RecipeResume";
import { BackupButton } from "../components/BackupControls";
import { BottleOptionsFields } from "../components/BottleOptionsFields";
import { GraphicsSection } from "../components/GraphicsSection";
import { KeyValueEditor } from "../components/KeyValueEditor";
import { LogsTab } from "../components/LogsTab";
import { ProgramsTab } from "../components/ProgramsTab";
import { ComponentsSection } from "../components/ComponentsSection";
import { SnapshotsSection } from "../components/SnapshotsSection";
import { ProgressBar } from "../components/ProgressBar";
import type { Route } from "../components/Sidebar";
import { Button, ErrorText, Label, field, subtle } from "../components/ui";
import { Select } from "../components/Select";

type Tab = "programs" | "tools" | "logs" | "settings";

export function BottleView({ id, onNavigate }: { id: string; onNavigate: (r: Route) => void }) {
  const { data: bottle, error } = useBottle(id);
  const { data: list } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const jobs = useJobs();
  const [tab, setTab] = useState<Tab>("programs");
  const [logToShow, setLogToShow] = useState<string>();

  const status = list?.find((b) => b.id === id)?.status;
  const setup = jobs.job(bottleJobKey(id));
  const runtime = runtimes?.find((r) => r.id === bottle?.runtime);
  const { data: recipeState } = useRecipeState(id);

  if (error) return <Page><ErrorText>{errorMessage(error)}</ErrorText></Page>;
  if (!bottle) return null;

  return (
    <Page>
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight">{bottle.name}</h1>
        <p className={subtle}>
          {runtime?.name ?? bottle.runtime} · {windowsLabel(bottle.windows_version)}
        </p>
      </header>

      {status === "setting_up" || (setup && isRunning(setup)) ? (
        <SetupProgress step={setup?.step} message={setup?.message} />
      ) : recipeState ? (
        <RecipeResume bottleId={bottle.id} state={recipeState} />
      ) : setup?.done && !setup.done.ok ? (
        <Notice>
          <p className="font-medium">Setting up this bottle failed.</p>
          <pre className="mt-2 max-h-48 overflow-auto text-xs whitespace-pre-wrap select-text">
            {setup.done.error}
          </pre>
        </Notice>
      ) : status === "incomplete" ? (
        <Notice>
          <p>
            Setting up this bottle was interrupted, for example because Vinter quit. Delete it and
            create it again.
          </p>
          <DeleteButton bottle={bottle} onDeleted={() => onNavigate({ page: "home" })} />
        </Notice>
      ) : (
        <>
          {!runtime && runtimes && (
            <Notice>
              This bottle uses {bottle.runtime}, which is not installed. Pick another runtime in
              Settings, or install it again.
            </Notice>
          )}
          <nav className="flex gap-1 border-b border-frost-200 dark:border-slate-deep-700">
            {(["programs", "tools", "logs", "settings"] as Tab[]).map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => setTab(t)}
                className={`-mb-px border-b-2 px-3 py-2 text-sm capitalize ${
                  tab === t
                    ? "border-wine-500 font-medium"
                    : "border-transparent text-slate-deep-700/70 hover:text-slate-deep-900 dark:text-frost-300/70 dark:hover:text-frost-100"
                }`}
              >
                {t}
              </button>
            ))}
          </nav>
          {tab === "programs" && (
            <ProgramsTab
              bottleId={bottle.id}
              needsRosetta={runtime?.needs_rosetta ?? false}
              onShowLog={(id) => {
                setLogToShow(id);
                setTab("logs");
              }}
            />
          )}
          {tab === "logs" && <LogsTab key={logToShow} bottleId={bottle.id} initial={logToShow} />}
          {tab === "tools" && <Tools bottle={bottle} needsRosetta={runtime?.needs_rosetta ?? false} />}
          {tab === "settings" && (
            <Settings
              bottle={bottle}
              needsRosetta={runtime?.needs_rosetta ?? false}
              onNavigate={onNavigate}
              onShowLog={(id) => {
                setLogToShow(id);
                setTab("logs");
              }}
            />
          )}
        </>
      )}
    </Page>
  );
}

function Page({ children }: { children: React.ReactNode }) {
  return <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">{children}</div>;
}

function Notice({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col items-start gap-3 rounded-xl border border-amber-300/60 bg-amber-50 p-4 text-sm dark:border-amber-500/30 dark:bg-amber-500/10">
      {children}
    </div>
  );
}

function SetupProgress({ step, message }: { step?: string; message?: string }) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-frost-200 p-4 dark:border-slate-deep-700">
      <p className="text-sm font-medium">Setting up this bottle…</p>
      <ProgressBar percent={null} />
      <p className={subtle}>
        {step && step !== "Starting" ? `${step}. ` : ""}
        {message ? `${message}. ` : ""}The first setup with a runtime can take a minute.
      </p>
    </div>
  );
}

function Tools({ bottle, needsRosetta }: { bottle: BottleDetails; needsRosetta: boolean }) {
  const ensureRosetta = useEnsureRosetta();
  const [error, setError] = useState<string>();
  const [started, setStarted] = useState<string>();
  const [stopping, setStopping] = useState(false);

  const run = async (label: string, action: () => Promise<unknown>, rosetta = true) => {
    setError(undefined);
    setStarted(undefined);
    if (rosetta && !(await ensureRosetta(needsRosetta))) return;
    try {
      await action();
      setStarted(label);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const openTool = (tool: Tool, label: string) =>
    run(`${label} is starting. Its window can take a few seconds to appear.`, () =>
      commands.bottleTool(bottle.id, tool),
    );

  return (
    <div className="flex flex-col gap-4">
      <ul className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {TOOLS.map((t) => (
          <li key={t.tool}>
            <button
              type="button"
              onClick={() => openTool(t.tool, t.label)}
              className="flex h-full w-full flex-col items-start gap-0.5 rounded-xl border border-frost-200 bg-white p-3 text-left hover:border-wine-500/60 dark:border-slate-deep-700 dark:bg-slate-deep-900"
            >
              <span className="text-sm font-medium">{t.label}</span>
              <span className={subtle}>{t.description}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => run("", () => commands.bottleOpenDriveC(bottle.id), false)}>
          <FolderOpen className="size-4" /> Open C: drive
        </Button>
        <Button
          disabled={stopping}
          onClick={async () => {
            setStopping(true);
            await run("Stopped every program in this bottle.", () => commands.bottleKillAll(bottle.id), false);
            setStopping(false);
          }}
        >
          <Square className="size-4" /> {stopping ? "Stopping…" : "Stop all programs"}
        </Button>
      </div>
      {started && <p className={subtle}>{started}</p>}
      {error && <ErrorText>{error}</ErrorText>}
      <ComponentsSection bottleId={bottle.id} />
    </div>
  );
}

function Settings({
  bottle,
  needsRosetta,
  onNavigate,
  onShowLog,
}: {
  bottle: BottleDetails;
  needsRosetta: boolean;
  onNavigate: (r: Route) => void;
  onShowLog: (logId: string) => void;
}) {
  const { data: runtimes } = useInstalledRuntimes();
  const update = useUpdateBottle(bottle.id);
  const updateOverrides = useUpdateBottle(bottle.id);
  const updateEnv = useUpdateBottle(bottle.id);
  const none = { name: null, runtime: null, env: null, dll_overrides: null };
  const jobs = useJobs();
  const queryClient = useQueryClient();
  const [name, setName] = useState(bottle.name);
  const [copyName, setCopyName] = useState(`${bottle.name} copy`);
  const [dupError, setDupError] = useState<string>();

  const winJob = jobs.job(windowsJobKey(bottle.id));
  const changingWindows = isRunning(winJob);
  const optJob = jobs.job(optionsJobKey(bottle.id));
  const changingOptions = isRunning(optJob);
  const { data: app } = useAppInfo();

  const duplicate = async () => {
    setDupError(undefined);
    try {
      const started = await commands.bottleDuplicate(bottle.id, copyName);
      jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
      onNavigate({ page: "bottle", id: started.bottle_id });
    } catch (e) {
      setDupError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <Label title="Name">
        <div className="flex gap-2">
          <input className={field} value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          <Button
            disabled={update.isPending || !name.trim() || name.trim() === bottle.name}
            onClick={() => update.mutate({ ...none, name })}
          >
            Rename
          </Button>
        </div>
      </Label>

      <Label title="Runtime">
        <Select
          value={bottle.runtime}
          disabled={update.isPending}
          onChange={(runtime) => update.mutate({ ...none, runtime })}
          options={[
            ...(runtimes?.some((r) => r.id === bottle.runtime)
              ? []
              : [{ value: bottle.runtime, label: `${bottle.runtime} (not installed)` }]),
            ...(runtimes?.map((r) => ({ value: r.id, label: r.name })) ?? []),
          ]}
        />
      </Label>
      {update.error && <ErrorText>{errorMessage(update.error)}</ErrorText>}

      <Label title="Windows version">
        <Select
          value={bottle.windows_version}
          disabled={changingWindows}
          options={WINDOWS_VERSIONS}
          onChange={(version) =>
            jobs.start(windowsJobKey(bottle.id), () => commands.bottleSetWindowsVersion(bottle.id, version))
          }
        />
      </Label>
      {changingWindows && <p className={subtle}>Applying the Windows version…</p>}
      {winJob?.done && !winJob.done.ok && <ErrorText>{winJob.done.error}</ErrorText>}

      <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <h2 className="text-sm font-medium">Display and keyboard</h2>
        <BottleOptionsFields
          value={bottle.options}
          os={app?.os}
          disabled={changingOptions}
          onChange={(options) =>
            jobs.start(optionsJobKey(bottle.id), () => commands.bottleSetOptions(bottle.id, options))
          }
        />
        <p className={subtle}>
          {changingOptions ? "Applying…" : "Running programs see changes the next time they start."}
        </p>
        {optJob?.done && !optJob.done.ok && <ErrorText>{optJob.done.error}</ErrorText>}
      </section>

      <div className="border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <GraphicsSection
          bottleId={bottle.id}
          current={bottle.graphics}
          needsRosetta={needsRosetta}
          onShowLog={onShowLog}
        />
      </div>

      <SnapshotsSection bottleId={bottle.id} />

      <BottleStorage bottleId={bottle.id} />

      <details className="flex flex-col gap-5 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <summary className="cursor-pointer text-sm font-medium">Advanced</summary>
        <div className="mt-4 flex flex-col gap-6">
          <KeyValueEditor
            title="DLL overrides"
            help="Which DLLs load Wine's version (b), the program's own (n), or n,b / b,n. Leave the mode empty to disable a DLL. These win over the graphics backend's."
            keyPlaceholder="d3d11"
            valuePlaceholder="n,b"
            values={bottle.dll_overrides}
            saving={updateOverrides.isPending}
            error={updateOverrides.error ? errorMessage(updateOverrides.error) : undefined}
            onSave={(dll_overrides) => updateOverrides.mutate({ ...none, dll_overrides })}
          />
          <KeyValueEditor
            title="Environment variables"
            help="Extra variables for every program in this bottle, applied last. Example: DXVK_HUD=fps shows a frame counter with DXVK."
            keyPlaceholder="DXVK_HUD"
            valuePlaceholder="fps"
            values={bottle.env}
            saving={updateEnv.isPending}
            error={updateEnv.error ? errorMessage(updateEnv.error) : undefined}
            onSave={(env) => updateEnv.mutate({ ...none, env })}
          />
        </div>
      </details>

      <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <h2 className="text-sm font-medium">Duplicate</h2>
        <p className={subtle}>
          Copies the whole bottle, including installed programs. Close its programs first.
        </p>
        <div className="flex gap-2">
          <input className={field} value={copyName} maxLength={64} onChange={(e) => setCopyName(e.target.value)} />
          <Button disabled={!copyName.trim()} onClick={duplicate}>
            <Copy className="size-4" /> Duplicate
          </Button>
        </div>
        {dupError && <ErrorText>{dupError}</ErrorText>}
      </section>

      <BackupButton bottleId={bottle.id} />

      <section className="flex flex-col items-start gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <h2 className="text-sm font-medium">Delete</h2>
        <p className={subtle}>
          Stops its programs and moves the bottle to the Trash, so you can still get it back.
        </p>
        <DeleteButton bottle={bottle} onDeleted={() => onNavigate({ page: "home" })} />
      </section>
    </div>
  );
}

function DeleteButton({ bottle, onDeleted }: { bottle: BottleDetails; onDeleted: () => void }) {
  const remove = useDeleteBottle();
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="flex flex-col gap-2">
      {confirming ? (
        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            disabled={remove.isPending}
            onClick={() => remove.mutate(bottle.id, { onSuccess: onDeleted })}
          >
            <Trash2 className="size-4" />
            {remove.isPending ? "Moving to Trash…" : `Move “${bottle.name}” to Trash`}
          </Button>
          <Button disabled={remove.isPending} onClick={() => setConfirming(false)}>
            Cancel
          </Button>
        </div>
      ) : (
        <Button variant="danger" onClick={() => setConfirming(true)}>
          <AlertTriangle className="size-4" /> Delete bottle
        </Button>
      )}
      {remove.error && <ErrorText>{errorMessage(remove.error)}</ErrorText>}
    </div>
  );
}

/** The bottle's size, and cleaning up old logs and temp files. */
function BottleStorage({ bottleId }: { bottleId: string }) {
  const queryClient = useQueryClient();
  const { data: bytes } = useQuery({
    queryKey: ["bottles", bottleId, "size"],
    queryFn: () => commands.bottleDiskUsage(bottleId),
  });
  const [result, setResult] = useState<string>();
  const [error, setError] = useState<string>();
  const [cleaning, setCleaning] = useState(false);

  const clean = async () => {
    setCleaning(true);
    setError(undefined);
    try {
      const freed = await commands.bottleClean(bottleId);
      setResult(freed > 0 ? `Freed ${formatBytes(freed)}.` : "Nothing to clean up.");
      await queryClient.invalidateQueries({ queryKey: ["bottles", bottleId, "size"] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCleaning(false);
    }
  };

  return (
    <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">Storage</h2>
      <p className={subtle}>
        This bottle uses {bytes === undefined ? "…" : formatBytes(bytes)}. Cleaning up removes old logs and
        Windows' temporary files; programs and their settings stay.
      </p>
      <div className="flex items-center gap-3">
        <Button onClick={clean} disabled={cleaning}>
          <Eraser className="size-4" /> Clean up
        </Button>
        {result && <span className={subtle}>{result}</span>}
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}
