import { useState } from "react";
import { AppWindow, LoaderCircle, Pencil, Pin, PinOff, Play, Plus, Search, Square, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { pinLabel, useAppInfo } from "../api/app";
import { commands, type PeArch, type ProgramView } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { installerJobKey, usePrograms, useRemoveProgram } from "../api/programs";
import { useEnsureRosetta } from "../api/rosetta";
import { latestRun, useRuns } from "../api/runs";
import { EditProgramDialog } from "./EditProgramDialog";
import { InstallDialog, ProgramIcon } from "./InstallDialog";
import { Button, ErrorText, subtle } from "./ui";

export function ProgramsTab({
  bottleId,
  needsRosetta,
  onShowLog,
}: {
  bottleId: string;
  needsRosetta: boolean;
  onShowLog: (logId: string) => void;
}) {
  const { data: programs } = usePrograms(bottleId);
  const jobs = useJobs();
  const installing = isRunning(jobs.job(installerJobKey(bottleId)));
  const [dialog, setDialog] = useState<"choose" | "pick" | null>(installing ? "choose" : null);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" onClick={() => setDialog("choose")}>
          {installing ? <LoaderCircle className="size-4 animate-spin" /> : <Plus className="size-4" />}
          {installing ? "Installer running…" : "Install a program…"}
        </Button>
        <Button onClick={() => setDialog("pick")} disabled={installing}>
          <Search className="size-4" /> Find installed programs
        </Button>
      </div>

      {programs?.length === 0 ? (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed border-frost-300 px-6 py-12 text-center dark:border-slate-deep-700">
          <AppWindow className="size-8 text-frost-400" />
          <p className="font-medium">No programs yet</p>
          <p className="max-w-sm text-sm text-slate-deep-700/70 dark:text-frost-300/70">
            Install a Windows program with its installer, or let Vinter find programs that are
            already installed in this bottle.
          </p>
        </div>
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3">
          {programs?.map((p) => (
            <ProgramTile
              key={`${p.index}-${p.exe}`}
              bottleId={bottleId}
              program={p}
              needsRosetta={needsRosetta}
              onShowLog={onShowLog}
            />
          ))}
        </ul>
      )}

      {dialog && (
        <InstallDialog
          bottleId={bottleId}
          needsRosetta={needsRosetta}
          start={dialog}
          onClose={() => setDialog(null)}
        />
      )}
    </div>
  );
}

export function ProgramTile({
  bottleId,
  program,
  needsRosetta,
  onShowLog,
  bottle,
}: {
  bottleId: string;
  program: ProgramView;
  needsRosetta: boolean;
  onShowLog: (logId: string) => void;
  /** Shown under the name (on the home page), with a link to the bottle. */
  bottle?: { name: string; open: () => void };
}) {
  const { runs, add } = useRuns();
  const ensureRosetta = useEnsureRosetta();
  const remove = useRemoveProgram(bottleId);
  const [error, setError] = useState<string>();
  const [starting, setStarting] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [editing, setEditing] = useState(false);
  const { data: app } = useAppInfo();
  const queryClient = useQueryClient();

  const togglePin = async () => {
    setError(undefined);
    try {
      await commands.programSetPinned(bottleId, program.index, !program.pinned);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const run = latestRun(runs, bottleId, program.index);
  const running = !!run && !run.exit;
  const failed = run?.exit && run.exit.code !== 0 && run.exit.code !== null;

  const launch = async () => {
    setError(undefined);
    if (!(await ensureRosetta(needsRosetta))) return;
    setStarting(true);
    try {
      add(await commands.programLaunch(bottleId, program.index));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setStarting(false);
    }
  };

  return (
    <li className="group relative flex flex-col items-center gap-2 rounded-xl border border-frost-200 bg-white p-4 text-center dark:border-slate-deep-700 dark:bg-slate-deep-900">
      <button
        type="button"
        title="Edit name, arguments and icon"
        aria-label="Edit"
        onClick={() => setEditing(true)}
        className="absolute top-1.5 right-7 rounded p-0.5 text-frost-400 opacity-0 group-hover:opacity-100 hover:bg-frost-100 dark:hover:bg-slate-deep-800"
      >
        <Pencil className="size-3.5" />
      </button>
      <button
        type="button"
        title="Remove from this bottle (keeps it installed)"
        onClick={() => setConfirmRemove(true)}
        className="absolute top-1.5 right-1.5 rounded p-0.5 text-frost-400 opacity-0 group-hover:opacity-100 hover:bg-frost-100 dark:hover:bg-slate-deep-800"
      >
        <X className="size-3.5" />
      </button>
      <button
        type="button"
        title={pinLabel(app?.os, program.pinned)}
        aria-label={pinLabel(app?.os, program.pinned)}
        aria-pressed={program.pinned}
        onClick={togglePin}
        className={`absolute top-1.5 left-1.5 rounded p-0.5 hover:bg-frost-100 dark:hover:bg-slate-deep-800 ${
          program.pinned ? "text-wine-500 dark:text-rose-300" : "text-frost-400 hover:text-wine-500 dark:hover:text-rose-300"
        }`}
      >
        {program.pinned ? <Pin className="size-3.5" /> : <PinOff className="size-3.5" />}
      </button>
      <ProgramIcon src={program.icon_data_url} size="size-12" />
      <span className="line-clamp-2 text-sm font-medium" title={program.exe}>
        {program.name}
      </span>
      {program.arch && <ArchBadge arch={program.arch} />}
      {bottle && (
        <button type="button" onClick={bottle.open} className={`${subtle} -mt-1.5 hover:underline`}>
          {bottle.name}
        </button>
      )}

      {confirmRemove ? (
        <div className="flex gap-1">
          <Button variant="danger" onClick={() => remove.mutate(program.index)}>
            Remove
          </Button>
          <Button onClick={() => setConfirmRemove(false)}>Keep</Button>
        </div>
      ) : running ? (
        <Button onClick={() => commands.programStop(run.info.run_id)}>
          <Square className="size-3.5" /> Stop
        </Button>
      ) : (
        <Button variant="primary" disabled={starting} onClick={launch}>
          {starting ? <LoaderCircle className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
          Open
        </Button>
      )}

      {running && <span className={subtle}>Running</span>}
      {failed && (
        <button type="button" className={`${subtle} underline`} onClick={() => onShowLog(run.info.log_id)}>
          Exited with code {run.exit?.code}. See log
        </button>
      )}
      {(error || remove.error) && <ErrorText>{error ?? errorMessage(remove.error)}</ErrorText>}
      {editing && <EditProgramDialog bottleId={bottleId} program={program} onClose={() => setEditing(false)} />}
    </li>
  );
}

const ARCH_BADGES: Record<PeArch, { label: string; title: string }> = {
  i386: { label: "32-bit", title: "A 32-bit Windows program. Runs in this 64-bit bottle through WoW64." },
  amd64: { label: "64-bit", title: "A 64-bit Intel (x86-64) Windows program." },
  arm64: { label: "ARM64", title: "Built for ARM. Runs natively in Apple Silicon bottles, without translation." },
  arm64ec: { label: "ARM64", title: "ARM64EC: ARM code that works with Intel code. Runs in Apple Silicon bottles." },
  arm64x: { label: "ARM64", title: "ARM64X: holds both ARM64 and ARM64EC code. Runs in Apple Silicon bottles." },
};

/** Which CPU a program is built for. */
function ArchBadge({ arch }: { arch: PeArch }) {
  const badge = ARCH_BADGES[arch];
  return (
    <span
      title={badge.title}
      className="-mt-1.5 rounded bg-frost-100 px-1.5 py-0.5 text-[10px] font-medium text-slate-deep-700 dark:bg-slate-deep-800 dark:text-frost-300"
    >
      {badge.label}
    </span>
  );
}
