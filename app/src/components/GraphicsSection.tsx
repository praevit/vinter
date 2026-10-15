import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, Download, Gauge, LoaderCircle, TriangleAlert } from "lucide-react";
import { commands, type BackendInfo, type Graphics, type GraphicsReport } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { useEnsureRosetta } from "../api/rosetta";
import { formatBytes } from "../api/runtimes";
import { ProgressBar } from "./ProgressBar";
import { Button, ErrorText, subtle } from "./ui";

export const graphicsJobKey = (bottleId: string) => `graphics:${bottleId}`;

/** Backend choice, download progress and "Test graphics" for one bottle. */
export function GraphicsSection({
  bottleId,
  current,
  needsRosetta,
  onShowLog,
}: {
  bottleId: string;
  current: Graphics;
  needsRosetta: boolean;
  onShowLog: (logId: string) => void;
}) {
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const backends = useQuery({
    queryKey: ["bottles", bottleId, "graphics"],
    queryFn: () => commands.graphicsBackends(bottleId),
  });
  const job = jobs.job(graphicsJobKey(bottleId));
  const switching = isRunning(job);
  const [report, setReport] = useState<GraphicsReport>();
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string>();

  const choose = (backend: Graphics) => {
    setReport(undefined);
    jobs.start(graphicsJobKey(bottleId), () => commands.bottleSetGraphics(bottleId, backend));
  };

  const test = async () => {
    setTestError(undefined);
    setReport(undefined);
    if (!(await ensureRosetta(needsRosetta))) return;
    setTesting(true);
    try {
      setReport(await commands.bottleTestGraphics(bottleId));
    } catch (e) {
      setTestError(errorMessage(e));
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="flex flex-col gap-3">
      <div>
        <h2 className="text-sm font-medium">Graphics</h2>
        <p className={subtle}>How Direct3D (DirectX) games and apps draw. Close this bottle's programs before switching.</p>
      </div>

      <ul className="flex flex-col gap-2" role="radiogroup" aria-label="Graphics backend">
        {backends.data?.map((b) => (
          <BackendOption
            key={b.backend}
            info={b}
            selected={b.backend === current}
            disabled={switching || !b.available}
            onSelect={() => choose(b.backend)}
          />
        ))}
      </ul>

      {switching && job && (
        <div className="flex flex-col gap-1">
          <ProgressBar percent={job.percent} />
          <p className={subtle}>
            {job.step}… {job.message}
          </p>
        </div>
      )}
      {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}

      <div className="flex flex-col items-start gap-2">
        <Button onClick={test} disabled={testing || switching}>
          {testing ? <LoaderCircle className="size-4 animate-spin" /> : <Gauge className="size-4" />}
          {testing ? "Testing…" : "Test graphics"}
        </Button>
        {testError && <ErrorText>{testError}</ErrorText>}
        {report && <Report report={report} onShowLog={onShowLog} />}
      </div>
    </section>
  );
}

function BackendOption({
  info,
  selected,
  disabled,
  onSelect,
}: {
  info: BackendInfo;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  const download =
    info.component && !info.component_installed
      ? `${formatBytes(info.component.size)} download`
      : null;
  return (
    <li>
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        disabled={disabled && !selected}
        onClick={selected ? undefined : onSelect}
        className={`flex w-full items-start gap-3 rounded-xl border p-3 text-left ${
          selected
            ? "border-wine-500 bg-wine-500/5"
            : "border-frost-200 hover:border-frost-400 disabled:opacity-60 disabled:hover:border-frost-200 dark:border-slate-deep-700"
        }`}
      >
        <span
          className={`mt-1 size-3.5 shrink-0 rounded-full border-2 ${
            selected ? "border-wine-500 bg-wine-500" : "border-frost-400"
          }`}
        />
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-2 text-sm font-medium">
            {info.name}
            {download && info.available && (
              <span className={`flex items-center gap-1 font-normal ${subtle}`}>
                <Download className="size-3" /> {download}
              </span>
            )}
          </span>
          <span className={`block ${subtle}`}>{info.reason ?? info.description}</span>
          {info.component && info.available && (
            <span className={`block ${subtle}`}>
              {info.component.name} · {info.component.license}
            </span>
          )}
        </span>
      </button>
    </li>
  );
}

function Report({ report, onShowLog }: { report: GraphicsReport; onShowLog: (id: string) => void }) {
  const good = report.ok && report.matches_backend;
  return (
    <div
      className={`flex w-full flex-col gap-1 rounded-xl border p-3 text-sm ${
        good
          ? "border-emerald-500/40 bg-emerald-500/5"
          : "border-amber-500/40 bg-amber-500/5"
      }`}
    >
      <p className="flex items-center gap-2 font-medium">
        {good ? (
          <CheckCircle2 className="size-4 text-emerald-600 dark:text-emerald-400" />
        ) : (
          <TriangleAlert className="size-4 text-amber-600 dark:text-amber-400" />
        )}
        {report.draws === false
          ? "Direct3D 11 started, but could not draw to a window."
          : !report.ok
          ? "Direct3D 11 did not start."
          : report.matches_backend
            ? `Direct3D 11 works with ${report.renderer}.`
            : `Direct3D 11 works, but through ${report.renderer ?? "another renderer"} instead of the chosen backend.`}
      </p>
      {report.ok && (
        <p className={subtle}>
          Feature level {report.feature_level ?? "unknown"} · {report.adapter ?? "unknown GPU"}
        </p>
      )}
      <button type="button" className={`self-start underline ${subtle}`} onClick={() => onShowLog(report.log_id)}>
        Show log
      </button>
    </div>
  );
}
