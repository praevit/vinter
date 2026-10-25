import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, LoaderCircle, Wrench } from "lucide-react";
import { commands, type ProgramView, type Rating } from "../api/bindings";
import { useBottle } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { logRoute, navigate } from "../api/navigate";
import { useEnsureRosetta } from "../api/rosetta";
import { useRuns } from "../api/runs";
import { useInstalledRuntimes } from "../api/runtimes";
import { Dialog } from "./Dialog";
import { DoctorPanel, doctorQueryKey } from "./DoctorPanel";
import { Button, ErrorText, field, subtle } from "./ui";

type Step =
  /** What the steps do, before anything runs. */
  | { kind: "start" }
  /** Running once with detailed logging, until it exits. */
  | { kind: "logging"; runId: string; logId: string }
  /** The log doctor's findings for that run. */
  | { kind: "findings"; logId: string; exitCode: number | null }
  /** Opened again after fixing; did it work? */
  | { kind: "retry"; runId: string; logId: string }
  | { kind: "done"; works: boolean };

/**
 * "Fix it" for one program: runs it once with detailed Wine logging (the
 * bottle's setting stays as it is), shows what the log doctor finds with its
 * one-click fixes, then tries again and asks whether that worked.
 */
export function FixItDialog({
  bottleId,
  program,
  onClose,
}: {
  bottleId: string;
  program: ProgramView;
  onClose: () => void;
}) {
  const { runs, add } = useRuns();
  const ensureRosetta = useEnsureRosetta();
  const { data: bottle } = useBottle(bottleId);
  const { data: runtimes } = useInstalledRuntimes();
  const needsRosetta = runtimes?.find((r) => r.id === bottle?.runtime)?.needs_rosetta ?? false;
  const [step, setStep] = useState<Step>({ kind: "start" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  // The last detailed log, whose findings go in the compatibility note.
  const [lastLog, setLastLog] = useState<string>();
  const { data: findings } = useQuery({
    queryKey: doctorQueryKey(bottleId, lastLog ?? ""),
    queryFn: () => commands.logDiagnose(bottleId, lastLog!),
    enabled: !!lastLog,
  });
  const found = findings?.map((f) => f.title) ?? [];

  const run = step.kind === "logging" || step.kind === "retry" ? runs.find((r) => r.info.run_id === step.runId) : undefined;
  // The detailed run ended: on to its findings.
  if (step.kind === "logging" && run?.exit) {
    setStep({ kind: "findings", logId: step.logId, exitCode: run.exit.code });
    setLastLog(step.logId);
  }

  const launch = async (logged: boolean) => {
    setError(undefined);
    if (!(await ensureRosetta(needsRosetta))) return;
    setBusy(true);
    try {
      const info = logged
        ? await commands.programLaunchLogged(bottleId, program.index, "verbose")
        : await commands.programLaunch(bottleId, program.index);
      add(info, { quiet: true });
      setStep({ kind: logged ? "logging" : "retry", runId: info.run_id, logId: info.log_id });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const running = !!run && !run.exit;

  return (
    <Dialog onClose={onClose}>
      <div className="flex max-h-[80vh] flex-col gap-4 overflow-y-auto text-left">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <Wrench className="size-5" /> Fix {program.name}
        </h2>

        {step.kind === "start" && (
          <>
            <p className="text-sm">
              Vinter opens {program.name} once with detailed logging, then looks through the log for known problems.
              Do whatever made it fail, then close it. The bottle's settings stay as they are.
            </p>
            <div className="flex justify-end gap-2">
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" disabled={busy} onClick={() => launch(true)}>
                {busy && <LoaderCircle className="size-4 animate-spin" />}
                Run it again with detailed logging
              </Button>
            </div>
          </>
        )}

        {step.kind === "logging" && (
          <>
            <p className="flex items-center gap-2 text-sm">
              <LoaderCircle className="size-4 shrink-0 animate-spin" />
              {program.name} is running with detailed logging. Do what made it fail, or close it.
            </p>
            <p className={subtle}>If it opens and works this time, you can stop here.</p>
            <div className="flex flex-wrap justify-end gap-2">
              <Button onClick={() => setStep({ kind: "done", works: true })}>It's working now</Button>
              <Button disabled={!running} onClick={() => run && commands.programStop(run.info.run_id)}>
                Close it and check the log
              </Button>
            </div>
          </>
        )}

        {step.kind === "findings" && (
          <>
            <p className="text-sm">
              {step.exitCode === null || step.exitCode === 0
                ? `${program.name} closed.`
                : `${program.name} exited with code ${step.exitCode}.`}{" "}
              Here is what the log shows. Apply a fix, then try again.
            </p>
            <DoctorPanel bottleId={bottleId} logId={step.logId} exitCode={step.exitCode} program={program} fixIt={false} />
            <div className="flex flex-wrap justify-end gap-2">
              <Button onClick={() => navigate(logRoute(bottleId, step.logId))}>Show log</Button>
              <Button variant="primary" disabled={busy} onClick={() => launch(false)}>
                {busy && <LoaderCircle className="size-4 animate-spin" />}
                Try again
              </Button>
            </div>
          </>
        )}

        {step.kind === "retry" && (
          <>
            <p className="text-sm">
              {running
                ? `${program.name} is open. Did that work?`
                : run?.exit && run.exit.code !== 0 && run.exit.code !== null
                  ? `${program.name} quit again (code ${run.exit.code}). Did that work?`
                  : `Did ${program.name} work?`}
            </p>
            <div className="flex flex-wrap justify-end gap-2">
              <Button onClick={() => setStep({ kind: "done", works: false })}>No</Button>
              <Button variant="primary" onClick={() => setStep({ kind: "done", works: true })}>
                Yes, it works
              </Button>
            </div>
          </>
        )}

        {step.kind === "done" && (
          <>
            {step.works ? (
              <p className="flex items-center gap-2 text-sm">
                <CheckCircle2 className="size-4 shrink-0 text-emerald-500" /> Good to hear it works.
              </p>
            ) : (
              <>
                <p className="text-sm">
                  Sorry it still fails. You can run it with detailed logging again after another fix, or look at
                  the log yourself. Copy debug report in the Logs tab makes a report to share.
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button disabled={busy} onClick={() => launch(true)}>
                    Run with detailed logging again
                  </Button>
                </div>
              </>
            )}
            <SaveNote bottleId={bottleId} program={program} works={step.works} found={found} />
            <div className="flex justify-end">
              <Button onClick={onClose}>Close</Button>
            </div>
          </>
        )}

        {error && <ErrorText>{error}</ErrorText>}
      </div>
    </Dialog>
  );
}

/** Offers to keep the outcome as a compatibility note. */
function SaveNote({
  bottleId,
  program,
  works,
  found,
}: {
  bottleId: string;
  program: ProgramView;
  works: boolean;
  found: string[];
}) {
  const queryClient = useQueryClient();
  const rating: Rating = works ? "works" : "broken";
  const [note, setNote] = useState(found.length ? `Log doctor found: ${found.join("; ")}.` : "");
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string>();

  const save = async () => {
    setError(undefined);
    try {
      await commands.compatNoteSet(bottleId, program.index, rating, note);
      await queryClient.invalidateQueries({ queryKey: ["compat-notes"] });
      setSaved(true);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <section className="flex flex-col gap-2 rounded-xl border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="text-sm font-medium">Save as a compatibility note?</h3>
      <textarea
        className={`${field} min-h-16`}
        value={note}
        maxLength={2000}
        placeholder="What you changed"
        onChange={(e) => setNote(e.target.value)}
      />
      <div className="flex items-center gap-2">
        <Button disabled={saved} onClick={save}>
          Save as “{works ? "Works" : "Broken"}”
        </Button>
        {saved && <span className={subtle}>Note saved.</span>}
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}

/** A small "Fix it…" link for a program that failed, opening the steps above. */
export function FixItButton({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className={`${subtle} underline`} onClick={() => setOpen(true)}>
        Fix it…
      </button>
      {open && <FixItDialog bottleId={bottleId} program={program} onClose={() => setOpen(false)} />}
    </>
  );
}
