import { useEffect, useState } from "react";
import { FileUp, LoaderCircle, RefreshCw, Square } from "lucide-react";
import { useAppInfo } from "../api/app";
import { commands, type DiscoveredProgram } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs, useQuietJob } from "../api/jobs";
import { installerJobKey, programPlace, useAddPrograms } from "../api/programs";
import { useEnsureRosetta } from "../api/rosetta";
import { AppReady } from "./AppReady";
import { Dialog } from "./Dialog";
import { Button, ErrorText, Label, field, subtle } from "./ui";

type Step = "choose" | "running" | "pick" | "ready";

/**
 * Installs a program into a bottle: choose an installer, run it, then pick
 * which of the programs found to add. Opens at "pick" for "Find programs".
 */
export function InstallDialog({
  bottleId,
  needsRosetta,
  start,
  initialPath,
  onClose,
}: {
  bottleId: string;
  needsRosetta: boolean;
  start: "choose" | "pick";
  /** An installer already chosen, e.g. a file opened with Vinter. */
  initialPath?: string;
  onClose: () => void;
}) {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const job = jobs.job(installerJobKey(bottleId));
  const [step, setStep] = useState<Step>(isRunning(job) ? "running" : start);
  const [path, setPath] = useState<string | undefined>(initialPath);
  const [args, setArgs] = useState("");
  const [error, setError] = useState<string>();
  const [stopping, setStopping] = useState(false);
  // The first program added, for the "is ready" step.
  const [added, setAdded] = useState<number>();
  const { data: app } = useAppInfo();
  // This dialog shows the installer's problems itself.
  useQuietJob(installerJobKey(bottleId));

  // When the installer exits, move on to picking programs; if it was
  // stopped from here, just close.
  useEffect(() => {
    if (step !== "running" || !job?.done) return;
    if (stopping) onClose();
    else setStep("pick");
  }, [step, job?.done, stopping, onClose]);

  const stop = async () => {
    setStopping(true);
    setError(undefined);
    try {
      await commands.bottleKillAll(bottleId);
    } catch (e) {
      setStopping(false);
      setError(errorMessage(e));
    }
  };

  const choose = async () => {
    setError(undefined);
    try {
      const picked = await commands.installerChoose();
      if (picked) setPath(picked);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const run = async () => {
    if (!path || !(await ensureRosetta(needsRosetta))) return;
    const argv = args.trim() ? args.trim().split(/\s+/) : [];
    await jobs.start(installerJobKey(bottleId), () => commands.installerRun(bottleId, path, argv));
    setStep("running");
  };

  const fileName = path?.split(/[\\/]/).pop();

  return (
    // While the installer runs, closing just hides this; it keeps running.
    <Dialog onClose={onClose}>
      {step === "choose" && (
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Install a program</h2>
          <p className="text-sm text-slate-deep-700/80 dark:text-frost-300/80">
            Choose the Windows installer you downloaded: an .exe, .msi or .msix file. It runs inside this
            bottle, and nothing is installed on your Mac itself.
          </p>
          <Button onClick={choose} className="self-start">
            <FileUp className="size-4" /> {fileName ? "Choose another…" : "Choose installer…"}
          </Button>
          {fileName && (
            <>
              <p className="truncate text-sm font-medium" title={path}>
                {fileName}
              </p>
              <details className="text-sm">
                <summary className="cursor-pointer text-frost-400">Advanced</summary>
                <div className="mt-2">
                  <Label title="Installer options">
                    <input
                      className={field}
                      value={args}
                      onChange={(e) => setArgs(e.target.value)}
                      placeholder="e.g. /S for a silent install"
                    />
                  </Label>
                </div>
              </details>
            </>
          )}
          {error && <ErrorText>{error}</ErrorText>}
          <div className="flex justify-end gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" disabled={!path} onClick={run}>
              Run installer
            </Button>
          </div>
        </div>
      )}

      {step === "running" && (
        <div className="flex flex-col items-center gap-3 py-4 text-center">
          <LoaderCircle className="size-8 animate-spin text-wine-500" />
          <h2 className="text-lg font-semibold">The installer is running</h2>
          <p className="text-sm text-slate-deep-700/80 dark:text-frost-300/80">
            Follow the steps in its window. When it finishes, Vinter looks for the programs it
            installed.
          </p>
          <p className={subtle}>
            Don't see its window? {app?.os === "macos" ? "Check the Dock." : "Check your taskbar."}
          </p>
          {error && <ErrorText>{error}</ErrorText>}
          <div className="mt-2 flex gap-2">
            <Button variant="danger" disabled={stopping} onClick={stop}>
              <Square className="size-4" /> {stopping ? "Stopping…" : "Stop installer"}
            </Button>
            <Button onClick={onClose}>Hide</Button>
          </div>
        </div>
      )}

      {step === "pick" && (
        <PickPrograms
          bottleId={bottleId}
          installerError={job?.done && !job.done.ok ? job.done.error : null}
          onAdded={(index) => {
            setAdded(index);
            setStep("ready");
          }}
          onClose={onClose}
        />
      )}

      {step === "ready" && (
        <AppReady bottleId={bottleId} index={added} needsRosetta={needsRosetta} onDone={onClose} />
      )}
    </Dialog>
  );
}

/** Steam games all run steam.exe, so a program is its exe plus its arguments. */
const programKey = (p: DiscoveredProgram) => [p.exe, ...p.args].join(" ");

function PickPrograms({
  bottleId,
  installerError,
  onAdded,
  onClose,
}: {
  bottleId: string;
  installerError: string | null;
  /** Called with the index of the first program added. */
  onAdded: (index: number) => void;
  onClose: () => void;
}) {
  const add = useAddPrograms(bottleId);
  const [found, setFound] = useState<DiscoveredProgram[]>();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string>();
  const [scanning, setScanning] = useState(false);

  const scan = async () => {
    setScanning(true);
    setError(undefined);
    try {
      const programs = await commands.programsDiscover(bottleId);
      setFound(programs);
      // Preselect new shortcuts; if there are none, preselect new executables.
      const fresh = programs.filter((p) => !p.already_added);
      const shortcuts = fresh.filter((p) => p.source === "shortcut");
      setSelected(new Set((shortcuts.length ? shortcuts : fresh).map(programKey)));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setScanning(false);
    }
  };

  // Scan once when this step opens.
  useEffect(() => {
    void scan();
  }, []);

  const toggle = (exe: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(exe)) next.delete(exe);
      else next.add(exe);
      return next;
    });

  const submit = () => {
    const chosen = found?.filter((p) => selected.has(programKey(p)) && !p.already_added) ?? [];
    add.mutate(chosen, {
      onSuccess: (bottle) => onAdded(bottle.programs.length - chosen.length),
    });
  };

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-lg font-semibold">Programs in this bottle</h2>
      {installerError && (
        <p className={subtle}>The installer reported a problem: {installerError}</p>
      )}
      {scanning && !found ? (
        <p className="flex items-center gap-2 text-sm">
          <LoaderCircle className="size-4 animate-spin" /> Looking for programs…
        </p>
      ) : found?.length === 0 ? (
        <p className="text-sm text-slate-deep-700/80 dark:text-frost-300/80">
          No programs found. If the installer is still running, finish it and scan again.
        </p>
      ) : (
        <ul className="flex max-h-80 flex-col gap-1 overflow-y-auto">
          {found?.map((p) => (
            <li key={programKey(p)}>
              <label
                className={`flex items-center gap-3 rounded-lg px-2 py-1.5 ${
                  p.already_added ? "opacity-50" : "hover:bg-frost-100 dark:hover:bg-slate-deep-800"
                }`}
              >
                <input
                  type="checkbox"
                  className="accent-wine-500"
                  checked={p.already_added || selected.has(programKey(p))}
                  disabled={p.already_added}
                  onChange={() => toggle(programKey(p))}
                />
                <ProgramIcon src={p.icon_data_url} />
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5">
                    <span className="truncate text-sm font-medium">{p.name}</span>
                    {p.source === "steam" && (
                      <span
                        title="A game in this bottle's Steam library. It starts through Steam."
                        className="shrink-0 rounded bg-frost-100 px-1.5 py-0.5 text-[10px] font-medium text-slate-deep-700 dark:bg-slate-deep-800 dark:text-frost-300"
                      >
                        Steam
                      </span>
                    )}
                  </span>
                  <span className={`block truncate ${subtle}`} title={p.exe}>
                    {p.already_added ? "Already added" : p.source === "steam" ? "Starts through Steam" : programPlace(p.exe)}
                  </span>
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {(error || add.error) && <ErrorText>{error ?? errorMessage(add.error)}</ErrorText>}
      <div className="flex justify-between gap-2">
        <Button onClick={scan} disabled={scanning}>
          <RefreshCw className={`size-4 ${scanning ? "animate-spin" : ""}`} /> Scan again
        </Button>
        <div className="flex gap-2">
          <Button onClick={onClose}>Close</Button>
          <Button variant="primary" disabled={!selected.size || add.isPending} onClick={submit}>
            Add {selected.size || ""} {selected.size === 1 ? "program" : "programs"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProgramIcon({ src, size = "size-8" }: { src: string | null; size?: string }) {
  return src ? (
    <img src={src} alt="" className={`${size} shrink-0 object-contain`} draggable={false} />
  ) : (
    <div className={`${size} shrink-0 rounded-lg bg-frost-200 dark:bg-slate-deep-700`} />
  );
}
