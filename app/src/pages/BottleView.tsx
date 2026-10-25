import { useState, type KeyboardEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Copy, Eraser, FolderOpen, HardDriveDownload, MoreHorizontal, Square, Trash2 } from "lucide-react";
import { commands, type Readiness, type Tool } from "../api/bindings";
import { READINESS_LABELS, ReadinessDot } from "../components/ReadinessDot";
import { ExperimentalBadge } from "../components/ExperimentalBadge";
import { MigrateDialog } from "../components/MigrateDialog";
import { useExperimental } from "../api/experimental";
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
import { useRuns } from "../api/runs";
import { useEnsureRosetta } from "../api/rosetta";
import { useRecipeState } from "../api/recipes";
import { ENGINE_LABELS, formatBytes, runtimeLabel, useInstalledRuntimes } from "../api/runtimes";
import { RecipeResume } from "../components/RecipeResume";
import { BackupButton, startBackup } from "../components/BackupControls";
import { Dialog } from "../components/Dialog";
import { Menu } from "../components/Menu";
import { BottleOptionsFields } from "../components/BottleOptionsFields";
import { DomainSection } from "../components/DomainSection";
import { GraphicsSection } from "../components/GraphicsSection";
import { KeyValueEditor } from "../components/KeyValueEditor";
import { LogsTab } from "../components/LogsTab";
import { ProgramsTab } from "../components/ProgramsTab";
import { ComponentsSection } from "../components/ComponentsSection";
import { SnapshotsSection } from "../components/SnapshotsSection";
import { ProgressBar } from "../components/ProgressBar";
import type { BottleFocus, Route } from "../components/Sidebar";
import { Button, ErrorText, Label, field, subtle } from "../components/ui";
import { Select } from "../components/Select";

const TABS = ["programs", "tools", "logs", "settings", "backups"] as const;
type Tab = (typeof TABS)[number];
const TAB_LABELS: Record<Tab, string> = {
  programs: "Programs",
  tools: "Tools",
  logs: "Logs",
  settings: "Settings",
  backups: "Snapshots & backups",
};
const isTab = (t: string | null | undefined): t is Tab => TABS.includes(t as Tab);
/** The tab a focus opens: snapshots live with the backups. */
const focusTab = (focus: BottleFocus): Tab =>
  focus.tab === "logs" ? "logs" : focus.section === "snapshots" ? "backups" : "settings";

/** The tab last shown for a bottle (kept in this window's storage). */
const tabKey = (id: string) => `vinter.bottle-tab.${id}`;
function savedTab(id: string): Tab | undefined {
  try {
    const t = localStorage.getItem(tabKey(id));
    return isTab(t) ? t : undefined;
  } catch {
    return undefined;
  }
}
function saveTab(id: string, tab: Tab) {
  try {
    localStorage.setItem(tabKey(id), tab);
  } catch {
    // Not remembered; that is fine.
  }
}

export function BottleView({
  id,
  focus,
  onNavigate,
  initialTab,
}: {
  id: string;
  /** Opens a tab (and a log in it). Each new value applies once. */
  focus?: BottleFocus;
  onNavigate: (r: Route) => void;
  /** A tab to open on (e.g. "logs"), instead of the one shown last. */
  initialTab?: string;
}) {
  const { data: bottle, error } = useBottle(id);
  const { data: list } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const jobs = useJobs();
  const [tab, setTabState] = useState<Tab>(() =>
    focus ? focusTab(focus) : isTab(initialTab) ? initialTab : (savedTab(id) ?? "programs"),
  );
  const [logToShow, setLogToShow] = useState<string | undefined>(focus?.tab === "logs" ? focus.log : undefined);
  // A new focus (e.g. "Show log" in a toast) while this page is open.
  const [seenFocus, setSeenFocus] = useState(focus);
  if (focus !== seenFocus) {
    setSeenFocus(focus);
    if (focus) {
      setTabState(focusTab(focus));
      if (focus.tab === "logs") setLogToShow(focus.log);
    }
  }
  const [migrating, setMigrating] = useState(false);
  const canMigrate = useExperimental("bottle-migrate-arm64");

  const setTab = (t: Tab) => {
    setTabState(t);
    saveTab(id, t);
  };

  const listed = list?.find((b) => b.id === id);
  const status = listed?.status;
  const readiness = listed?.readiness;
  const setup = jobs.job(bottleJobKey(id));
  const runtime = runtimes?.find((r) => r.id === bottle?.runtime);
  const { data: recipeState } = useRecipeState(id);

  if (error) return <Page><ErrorText>{errorMessage(error)}</ErrorText></Page>;
  if (!bottle) return null;

  const settingUp = status === "setting_up" || (setup && isRunning(setup));
  const usable = !settingUp && !recipeState && !(setup?.done && !setup.done.ok) && status !== "incomplete";
  const showLog = (logId?: string) => {
    setLogToShow(logId);
    setTab("logs");
  };

  return (
    <Page>
      <header className="flex items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <h1 className="truncate text-2xl font-semibold tracking-tight">{bottle.name}</h1>
          <p className={subtle}>
            {runtime ? runtimeLabel(runtime) : bottle.runtime} · {windowsLabel(bottle.windows_version)}
          </p>
        </div>
        {usable && (
          <>
            <RunningChip bottleId={bottle.id} />
            <BottleMenu bottle={bottle} onNavigate={onNavigate} onShowTab={setTab} />
          </>
        )}
      </header>

      {settingUp ? (
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
          {readiness && (readiness.level === "yellow" || readiness.level === "red") && (
            <ReadinessNotice
              readiness={readiness}
              legacy32={listed?.legacy_32bit ?? false}
              onNewBottle={() => onNavigate({ page: "new-bottle" })}
              onRecreate={canMigrate ? () => setMigrating(true) : undefined}
            />
          )}
          {!runtime && runtimes && (
            <Notice>
              This bottle uses {bottle.runtime}, which is not installed. Pick another runtime in
              Settings, or install it again.
            </Notice>
          )}
          <Tabs tab={tab} onChange={setTab} />
          <div role="tabpanel" id={`bottle-panel-${tab}`} aria-labelledby={`bottle-tab-${tab}`}>
            {tab === "programs" && (
              <ProgramsTab
                bottleId={bottle.id}
                needsRosetta={runtime?.needs_rosetta ?? false}
                onShowLog={showLog}
              />
            )}
            {tab === "logs" && <LogsTab key={logToShow} bottleId={bottle.id} initial={logToShow} />}
            {tab === "tools" && <Tools bottle={bottle} needsRosetta={runtime?.needs_rosetta ?? false} />}
            {tab === "settings" && (
              <Settings bottle={bottle} needsRosetta={runtime?.needs_rosetta ?? false} onShowLog={showLog} />
            )}
            {tab === "backups" && <Backups bottle={bottle} onNavigate={onNavigate} />}
          </div>
        </>
      )}
      {migrating && (
        <MigrateDialog
          bottleId={bottle.id}
          bottleName={bottle.name}
          onClose={() => setMigrating(false)}
          onStarted={(id) => {
            setMigrating(false);
            onNavigate({ page: "bottle", id });
          }}
        />
      )}
    </Page>
  );
}

/** The bottle's tabs; arrow keys move between them. */
function Tabs({ tab, onChange }: { tab: Tab; onChange: (t: Tab) => void }) {
  const onKeyDown = (e: KeyboardEvent) => {
    const at = TABS.indexOf(tab);
    const next =
      e.key === "ArrowRight"
        ? TABS[(at + 1) % TABS.length]
        : e.key === "ArrowLeft"
          ? TABS[(at - 1 + TABS.length) % TABS.length]
          : e.key === "Home"
            ? TABS[0]
            : e.key === "End"
              ? TABS[TABS.length - 1]
              : undefined;
    if (!next) return;
    e.preventDefault();
    onChange(next);
    document.getElementById(`bottle-tab-${next}`)?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label="Bottle"
      onKeyDown={onKeyDown}
      className="flex gap-1 overflow-x-auto border-b border-frost-200 dark:border-slate-deep-700"
    >
      {TABS.map((t) => (
        <button
          key={t}
          id={`bottle-tab-${t}`}
          type="button"
          role="tab"
          aria-selected={tab === t}
          aria-controls={`bottle-panel-${t}`}
          tabIndex={tab === t ? 0 : -1}
          onClick={() => onChange(t)}
          className={`-mb-px shrink-0 border-b-2 px-3 py-2 text-sm whitespace-nowrap ${
            tab === t
              ? "border-wine-500 font-medium"
              : "border-transparent text-secondary hover:text-slate-deep-900 dark:hover:text-frost-100"
          }`}
        >
          {TAB_LABELS[t]}
        </button>
      ))}
    </div>
  );
}

/** "2 running · Stop all", while programs started from Vinter run in this bottle. */
function RunningChip({ bottleId }: { bottleId: string }) {
  const { runs } = useRuns();
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string>();
  const count = runs.filter((r) => r.info.bottle_id === bottleId && !r.exit).length;
  if (count === 0) return null;

  const stopAll = async () => {
    setStopping(true);
    setError(undefined);
    try {
      await commands.bottleKillAll(bottleId);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setStopping(false);
    }
  };

  return (
    <div
      className="flex shrink-0 items-center gap-2 rounded-full border border-frost-300 py-1 pr-1 pl-3 text-sm dark:border-slate-deep-700"
      title={error}
    >
      <span className="size-2 rounded-full bg-emerald-500" aria-hidden />
      <span>{count} running</span>
      <span className="text-tertiary" aria-hidden>
        ·
      </span>
      <button
        type="button"
        disabled={stopping}
        onClick={stopAll}
        className="flex items-center gap-1 rounded-full px-2 py-0.5 hover:bg-frost-100 disabled:opacity-50 dark:hover:bg-slate-deep-800"
      >
        <Square className="size-3" aria-hidden /> {stopping ? "Stopping…" : "Stop all"}
      </button>
      {error && <span className="sr-only">{error}</span>}
    </div>
  );
}

/** The "…" menu next to the bottle's name. */
function BottleMenu({
  bottle,
  onNavigate,
  onShowTab,
}: {
  bottle: BottleDetails;
  onNavigate: (r: Route) => void;
  onShowTab: (t: Tab) => void;
}) {
  const jobs = useJobs();
  const [dialog, setDialog] = useState<"duplicate" | "delete">();
  const [error, setError] = useState<string>();

  const backUp = async () => {
    setError(undefined);
    // Its progress shows on the Snapshots & backups tab.
    onShowTab("backups");
    setError(await startBackup(jobs, bottle.id));
  };
  const openDrive = () => {
    setError(undefined);
    commands.bottleOpenDriveC(bottle.id).catch((e) => setError(errorMessage(e)));
  };

  return (
    <>
      <Menu
        label="More actions"
        items={[
          { label: "Duplicate…", icon: <Copy className="size-4" aria-hidden />, onSelect: () => setDialog("duplicate") },
          { label: "Back up…", icon: <HardDriveDownload className="size-4" aria-hidden />, onSelect: backUp },
          { label: "Show C: drive", icon: <FolderOpen className="size-4" aria-hidden />, onSelect: openDrive },
          {
            label: "Move to Trash…",
            icon: <Trash2 className="size-4" aria-hidden />,
            onSelect: () => setDialog("delete"),
            danger: true,
            separatorBefore: true,
          },
        ]}
      >
        <MoreHorizontal className="size-4" />
      </Menu>
      {error && (
        <Dialog
          onClose={() => setError(undefined)}
          title="That did not work"
          actions={<Button onClick={() => setError(undefined)}>OK</Button>}
        >
          <ErrorText>{error}</ErrorText>
        </Dialog>
      )}
      {dialog === "duplicate" && (
        <DuplicateDialog bottle={bottle} onNavigate={onNavigate} onClose={() => setDialog(undefined)} />
      )}
      {dialog === "delete" && (
        <DeleteDialog
          bottle={bottle}
          onDeleted={() => onNavigate({ page: "home" })}
          onClose={() => setDialog(undefined)}
        />
      )}
    </>
  );
}

function DeleteDialog({
  bottle,
  onDeleted,
  onClose,
}: {
  bottle: BottleDetails;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const remove = useDeleteBottle();
  return (
    <Dialog
      onClose={remove.isPending ? undefined : onClose}
      title={`Move “${bottle.name}” to the Trash?`}
      actions={
        <>
          <Button disabled={remove.isPending} onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={remove.isPending}
            onClick={() => remove.mutate(bottle.id, { onSuccess: onDeleted })}
          >
            <Trash2 className="size-4" aria-hidden />
            {remove.isPending ? "Moving to Trash…" : "Move to Trash"}
          </Button>
        </>
      }
    >
      <p className="text-sm">
        Stops its programs and moves the bottle, with everything installed in it, to the Trash. You can still get it
        back from there.
      </p>
      {remove.error && <ErrorText>{errorMessage(remove.error)}</ErrorText>}
    </Dialog>
  );
}

function DuplicateDialog({
  bottle,
  onNavigate,
  onClose,
}: {
  bottle: BottleDetails;
  onNavigate: (r: Route) => void;
  onClose: () => void;
}) {
  const duplicate = useDuplicate(bottle, onNavigate);
  const [name, setName] = useState(`${bottle.name} copy`);
  const busy = duplicate.busy;
  const go = () => name.trim() && !busy && void duplicate.run(name);

  return (
    <Dialog
      onClose={busy ? undefined : onClose}
      title={`Duplicate “${bottle.name}”`}
      actions={
        <>
          <Button disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || !name.trim()} onClick={go}>
            <Copy className="size-4" aria-hidden /> Duplicate
          </Button>
        </>
      }
    >
      <p className={subtle}>Copies the whole bottle, including installed programs. Close its programs first.</p>
      <Label title="Name of the copy">
        <input
          className={field}
          value={name}
          maxLength={64}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && go()}
        />
      </Label>
      {duplicate.error && <ErrorText>{duplicate.error}</ErrorText>}
    </Dialog>
  );
}

/** Copies a bottle, then shows the copy (which sets itself up). */
function useDuplicate(bottle: BottleDetails, onNavigate: (r: Route) => void) {
  const jobs = useJobs();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const run = async (name: string) => {
    setBusy(true);
    setError(undefined);
    try {
      const started = await commands.bottleDuplicate(bottle.id, name);
      jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
      onNavigate({ page: "bottle", id: started.bottle_id });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return { run, error, busy };
}

function Page({ children }: { children: React.ReactNode }) {
  return <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">{children}</div>;
}

/** Why a bottle depends on Rosetta, and what to do about it. */
function ReadinessNotice({
  readiness,
  legacy32,
  onNewBottle,
  onRecreate,
}: {
  readiness: Readiness;
  legacy32: boolean;
  onNewBottle: () => void;
  onRecreate?: () => void;
}) {
  return (
    <div className="flex items-start gap-3 rounded-xl border border-frost-200 p-4 text-sm dark:border-slate-deep-700">
      <span className="mt-1.5 flex">
        <ReadinessDot readiness={readiness} />
      </span>
      <div className="flex flex-1 flex-col items-start gap-2">
        <p>
          <span className="font-medium">{legacy32 ? "Legacy 32-bit bottle" : READINESS_LABELS[readiness.level]}.</span>{" "}
          {readiness.summary}
        </p>
        <div className="flex flex-wrap gap-2">
          {(readiness.level === "yellow" || legacy32) && <Button onClick={onNewBottle}>New Apple Silicon bottle…</Button>}
          {readiness.level === "yellow" && onRecreate && (
            <Button onClick={onRecreate}>
              Recreate for Apple Silicon… <ExperimentalBadge />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
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
  onShowLog,
}: {
  bottle: BottleDetails;
  needsRosetta: boolean;
  onShowLog: (logId: string) => void;
}) {
  const { data: runtimes } = useInstalledRuntimes();
  const update = useUpdateBottle(bottle.id);
  const updateOverrides = useUpdateBottle(bottle.id);
  const updateEnv = useUpdateBottle(bottle.id);
  const none = { name: null, runtime: null, env: null, dll_overrides: null };
  const jobs = useJobs();
  const [name, setName] = useState(bottle.name);

  const winJob = jobs.job(windowsJobKey(bottle.id));
  const changingWindows = isRunning(winJob);
  const optJob = jobs.job(optionsJobKey(bottle.id));
  const changingOptions = isRunning(optJob);
  const { data: app } = useAppInfo();

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
            ...(runtimes
              ?.filter((r) => r.engine === bottle.engine)
              .map((r) => ({ value: r.id, label: runtimeLabel(r) })) ?? []),
          ]}
        />
        <span className={subtle}>
          Made for the {ENGINE_LABELS[bottle.engine]} engine. A bottle keeps its engine, so only runtimes for it are
          listed.
        </span>
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

      <DomainSection
        key={bottle.id}
        bottle={bottle}
        runtime={runtimes?.find((r) => r.id === bottle.runtime)}
        os={app?.os}
      />

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
    </div>
  );
}

/** Snapshots, backups, copies and disk space. */
function Backups({ bottle, onNavigate }: { bottle: BottleDetails; onNavigate: (r: Route) => void }) {
  const duplicate = useDuplicate(bottle, onNavigate);
  const [copyName, setCopyName] = useState(`${bottle.name} copy`);

  return (
    <div className="flex flex-col gap-6 [&>section:first-child]:border-t-0 [&>section:first-child]:pt-0">
      <SnapshotsSection bottleId={bottle.id} />

      <BackupButton bottleId={bottle.id} />

      <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
        <h2 className="text-sm font-medium">Duplicate</h2>
        <p className={subtle}>
          Copies the whole bottle, including installed programs. Close its programs first.
        </p>
        <div className="flex gap-2">
          <input
            className={field}
            value={copyName}
            maxLength={64}
            aria-label="Name of the copy"
            onChange={(e) => setCopyName(e.target.value)}
          />
          <Button disabled={!copyName.trim() || duplicate.busy} onClick={() => duplicate.run(copyName)}>
            <Copy className="size-4" /> Duplicate
          </Button>
        </div>
        {duplicate.error && <ErrorText>{duplicate.error}</ErrorText>}
      </section>

      <BottleStorage bottleId={bottle.id} />
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
