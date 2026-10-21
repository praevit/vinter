import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, Stethoscope, XCircle } from "lucide-react";
import { commands, type Finding } from "../api/bindings";
import { useJobs, isRunning } from "../api/jobs";
import { graphicsJobKey } from "./GraphicsSection";
import { Button, ErrorText, subtle } from "./ui";

const BACKEND_NAMES = { wined3d: "WineD3D", dxvk: "DXVK", dxmt: "DXMT", d3dmetal: "D3DMetal" };

/** Known problems in a saved log, with how to fix them. Shows nothing if there are none. */
export function DoctorPanel({ bottleId, logId }: { bottleId: string; logId: string }) {
  const { data: findings } = useQuery({
    queryKey: ["bottles", bottleId, "logs", logId, "doctor"],
    queryFn: () => commands.logDiagnose(bottleId, logId),
  });
  if (!findings?.length) return null;
  return (
    <section className="flex max-h-56 flex-col gap-2 overflow-y-auto rounded-lg border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="flex items-center gap-1.5 text-sm font-medium">
        <Stethoscope className="size-4" /> Log doctor
      </h3>
      {findings.map((f, i) => (
        <FindingRow key={`${f.id}-${i}`} bottleId={bottleId} finding={f} />
      ))}
    </section>
  );
}

function FindingRow({ bottleId, finding }: { bottleId: string; finding: Finding }) {
  const Icon = finding.severity === "error" ? XCircle : AlertTriangle;
  return (
    <div className="flex flex-col gap-1">
      <span className="flex items-center gap-1.5 text-sm">
        <Icon className="size-4 shrink-0 text-wine-500 dark:text-rose-400" />
        {finding.title}
      </span>
      <p className={subtle}>{finding.detail}</p>
      <FixAction bottleId={bottleId} finding={finding} />
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

function FixAction({ bottleId, finding }: { bottleId: string; finding: Finding }) {
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
        </div>
      );
    }
    case "components":
      return <p className={subtle}>Fix: install {fix.verbs.join(", ")}.</p>;
    case "restore_snapshot":
      return <p className={subtle}>Fix: restore a snapshot in Settings → Snapshots.</p>;
    case "manual":
      return null;
  }
}
