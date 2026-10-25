import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Stethoscope, Wrench, XCircle } from "lucide-react";
import { commands, type Finding, type ProgramView } from "../api/bindings";
import { optionsJobKey, useBottle } from "../api/bottles";
import { useExperimental } from "../api/experimental";
import { useJobs, isRunning } from "../api/jobs";
import { navigate } from "../api/navigate";
import { useLogs, usePrograms } from "../api/programs";
import { useEnsureRosetta } from "../api/rosetta";
import { CompareDialog } from "./CompareDialog";
import { CONSENT, JobProgress, useInstallComponents } from "./ComponentsSection";
import { FixItDialog } from "./FixItDialog";
import { graphicsJobKey } from "./GraphicsSection";
import { Button, ErrorText, subtle } from "./ui";

const BACKEND_NAMES = { wined3d: "WineD3D", dxvk: "DXVK", dxmt: "DXMT", d3dmetal: "D3DMetal" };

export const doctorQueryKey = (bottleId: string, logId: string) => ["bottles", bottleId, "logs", logId, "doctor"];

/**
 * Known problems in a saved log, with how to fix them. Says so when it found
 * nothing, and when the bottle's Wine logging is off so it could not see much.
 */
export function DoctorPanel({
  bottleId,
  logId,
  exitCode,
  program: given,
  fixIt = true,
}: {
  bottleId: string;
  logId: string;
  /** How the run ended, if known: null when it was stopped. */
  exitCode?: number | null;
  /** The program the log is from; found by the log's name if not given. */
  program?: ProgramView;
  /** Offer the "Fix it" steps for the program. */
  fixIt?: boolean;
}) {
  const { data: findings } = useQuery({
    queryKey: doctorQueryKey(bottleId, logId),
    queryFn: () => commands.logDiagnose(bottleId, logId),
  });
  const { data: bottle } = useBottle(bottleId);
  const found = useLogProgram(bottleId, logId);
  const program = given ?? found;
  const [fixing, setFixing] = useState(false);
  if (!findings) return null;

  const failed = exitCode !== undefined && exitCode !== null && exitCode !== 0;
  const loggingOff = bottle?.options.wine_debug === "off" && (failed || findings.length === 0);

  return (
    <section className="flex max-h-56 flex-col gap-2 overflow-y-auto rounded-lg border border-frost-200 p-3 dark:border-slate-deep-700">
      <div className="flex items-center justify-between gap-2">
        <h3 className="flex items-center gap-1.5 text-sm font-medium">
          <Stethoscope className="size-4" /> Log doctor
        </h3>
        {fixIt && program && (
          <Button onClick={() => setFixing(true)} title="Run it again with detailed logging, then fix what it finds">
            <Wrench className="size-3.5" /> Fix it…
          </Button>
        )}
      </div>
      {loggingOff && <LoggingOff bottleId={bottleId} />}
      {findings.length === 0 && !loggingOff && (
        <p className={`${subtle} flex items-center gap-1.5`}>
          <CheckCircle2 className="size-3.5 text-emerald-500" /> No known problems found.
        </p>
      )}
      {findings.map((f, i) => (
        <FindingRow key={`${f.id}-${i}`} bottleId={bottleId} program={program} finding={f} />
      ))}
      {fixing && program && <FixItDialog bottleId={bottleId} program={program} onClose={() => setFixing(false)} />}
    </section>
  );
}

/** The program a log belongs to: logs are named after the program, with odd characters as spaces. */
function useLogProgram(bottleId: string, logId: string): ProgramView | undefined {
  const { data: logs } = useLogs(bottleId);
  const { data: programs } = usePrograms(bottleId);
  const label = logs?.find((l) => l.id === logId)?.label;
  if (!label) return undefined;
  return programs?.find((p) => p.name.replace(/[^A-Za-z0-9-]/g, " ") === label);
}

/** The bottle logs nothing from Wine, so most problems cannot show up. */
function LoggingOff({ bottleId }: { bottleId: string }) {
  const { data: bottle } = useBottle(bottleId);
  const jobs = useJobs();
  const job = jobs.job(optionsJobKey(bottleId));
  if (!bottle) return null;
  return (
    <div className="flex flex-col items-start gap-2 rounded-md border border-amber-300/60 bg-amber-50 p-2.5 text-sm dark:border-amber-500/30 dark:bg-amber-500/10">
      <p>
        Wine logging is off for this bottle, so the log doctor can't see much. Turn on error logging, then open the
        program again for a log it can read.
      </p>
      <Button
        disabled={isRunning(job)}
        onClick={() =>
          jobs.start(optionsJobKey(bottleId), () =>
            commands.bottleSetOptions(bottleId, { ...bottle.options, wine_debug: "errors" }),
          )
        }
      >
        {isRunning(job) ? "Turning on…" : "Turn on error logging"}
      </Button>
      {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}
    </div>
  );
}

function FindingRow({ bottleId, program, finding }: { bottleId: string; program?: ProgramView; finding: Finding }) {
  const Icon = finding.severity === "error" ? XCircle : AlertTriangle;
  return (
    <div className="flex flex-col gap-1">
      <span className="flex items-center gap-1.5 text-sm">
        <Icon className="size-4 shrink-0 text-wine-500 dark:text-rose-400" />
        {finding.title}
      </span>
      <p className={subtle}>{finding.detail}</p>
      <FixAction bottleId={bottleId} program={program} finding={finding} />
      {finding.evidence.length > 0 && (
        <details>
          <summary className={`${subtle} cursor-pointer`}>Log lines</summary>
          <pre className="mt-1 font-mono text-xs whitespace-pre-wrap select-text">
            {finding.evidence.join("\n")}
          </pre>
        </details>
      )}
    </div>
  );
}

function FixAction({ bottleId, program, finding }: { bottleId: string; program?: ProgramView; finding: Finding }) {
  const jobs = useJobs();
  const fix = finding.fix;
  switch (fix.kind) {
    case "graphics": {
      const job = jobs.job(graphicsJobKey(bottleId));
      return (
        <div className="flex flex-col items-start gap-1">
          <Button
            disabled={isRunning(job)}
            onClick={() =>
              jobs.start(graphicsJobKey(bottleId), () => commands.bottleSetGraphics(bottleId, fix.backend))
            }
          >
            Switch to {BACKEND_NAMES[fix.backend]}
          </Button>
          {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}
          {job?.done?.ok && <p className={subtle}>Switched. Open the program again to try it.</p>}
        </div>
      );
    }
    case "components":
      return <InstallFix bottleId={bottleId} verbs={fix.verbs} />;
    case "restore_snapshot":
      return (
        <div>
          <Button
            onClick={() => navigate({ page: "bottle", id: bottleId, focus: { tab: "settings", section: "snapshots" } })}
          >
            Open snapshots
          </Button>
        </div>
      );
    case "open_runtimes":
      return <RuntimesButton />;
    case "install_rosetta":
      return <RosettaFix />;
    case "other_engine":
      return <OtherEngineFix bottleId={bottleId} program={program} />;
    case "manual":
      return null;
  }
}

function RuntimesButton() {
  return (
    <div>
      <Button onClick={() => navigate({ page: "runtimes" })}>Open Runtimes</Button>
    </div>
  );
}

function RosettaFix() {
  const ensureRosetta = useEnsureRosetta();
  const [done, setDone] = useState(false);
  return (
    <div className="flex flex-col items-start gap-1">
      <Button onClick={async () => setDone(await ensureRosetta(true))}>Install Rosetta</Button>
      {done && <p className={subtle}>Rosetta is installed. Open the program again.</p>}
    </div>
  );
}

/** Try the program in a bottle on the other engine, or on a newer runtime. */
function OtherEngineFix({ bottleId, program }: { bottleId: string; program?: ProgramView }) {
  const canCompare = useExperimental("engine-ab-compare");
  const { data: bottle } = useBottle(bottleId);
  const [comparing, setComparing] = useState(false);
  return (
    <div className="flex flex-wrap gap-2">
      {canCompare && program && (
        <Button onClick={() => setComparing(true)}>
          {bottle?.engine === "arm64-fex" ? "Compare with an Intel bottle…" : "Compare with another bottle…"}
        </Button>
      )}
      <Button onClick={() => navigate({ page: "runtimes" })}>Open Runtimes</Button>
      {comparing && program && (
        <CompareDialog bottleId={bottleId} program={program} onClose={() => setComparing(false)} />
      )}
    </div>
  );
}

function InstallFix({ bottleId, verbs }: { bottleId: string; verbs: string[] }) {
  const { install, job, error } = useInstallComponents(bottleId);
  return (
    <div className="flex flex-col items-start gap-1">
      <Button disabled={isRunning(job)} onClick={() => install(verbs)}>
        Agree and install {verbs.join(", ")}
      </Button>
      <p className={subtle}>{CONSENT}</p>
      <JobProgress job={job} />
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
