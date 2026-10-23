import { useEffect, useRef, useState } from "react";
import { ClipboardCopy, FileArchive, RefreshCw } from "lucide-react";
import { commands } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { useLog, useLogs } from "../api/programs";
import { useRuns } from "../api/runs";
import { DoctorPanel } from "./DoctorPanel";
import { Button, ErrorText, subtle } from "./ui";

/** Run history of a bottle, with live output for programs still running. */
export function LogsTab({ bottleId, initial }: { bottleId: string; initial?: string }) {
  const logs = useLogs(bottleId);
  const { runs } = useRuns();
  const [selected, setSelected] = useState<string | undefined>(initial);
  const current = selected ?? logs.data?.[0]?.id;

  const liveRun = runs.find((r) => r.info.bottle_id === bottleId && r.info.log_id === current && !r.exit);
  const file = useLog(bottleId, current, !!liveRun);
  const text = liveRun ? liveRun.lines.map((l) => l.line).join("\n") : (file.data ?? "");

  const view = useRef<HTMLPreElement>(null);
  useEffect(() => {
    // Follow the end of the log, like `tail -f`.
    view.current?.scrollTo({ top: view.current.scrollHeight });
  }, [text]);

  const [copied, setCopied] = useState<string>();
  const copyReport = async () => {
    try {
      await navigator.clipboard.writeText(await commands.debugReport(bottleId));
      setCopied("Debug report copied. Paste it into a bug report.");
    } catch (e) {
      setCopied(`Could not copy: ${String(e)}`);
    }
  };

  const exportDiagnostics = async () => {
    try {
      const path = await commands.diagExport(bottleId);
      if (path) setCopied(`Diagnostics saved to ${path}. Attach the file to a bug report.`);
    } catch (e) {
      setCopied(`Could not export: ${errorMessage(e)}`);
    }
  };

  const isLive = (id: string) =>
    runs.some((r) => r.info.bottle_id === bottleId && r.info.log_id === id && !r.exit);

  const reportBar = (
    <div className="flex items-center gap-3">
      <Button onClick={copyReport}>
        <ClipboardCopy className="size-4" /> Copy debug report
      </Button>
      <Button
        onClick={exportDiagnostics}
        title="A zip of this bottle's logs, settings and system checks, with your home folder hidden. No documents from its C: drive."
      >
        <FileArchive className="size-4" /> Export diagnostics
      </Button>
      {copied && <span className={subtle}>{copied}</span>}
    </div>
  );

  if (logs.data?.length === 0) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm text-frost-400">No logs yet. Every program you open gets one.</p>
        {reportBar}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
    {reportBar}
    <div className="flex h-[28rem] gap-3">
      <ul className="flex w-56 shrink-0 flex-col gap-0.5 overflow-y-auto">
        {logs.data?.map((l) => (
          <li key={l.id}>
            <button
              type="button"
              onClick={() => setSelected(l.id)}
              className={`flex w-full flex-col rounded-md px-2 py-1.5 text-left ${
                current === l.id
                  ? "bg-frost-200 dark:bg-slate-deep-800"
                  : "hover:bg-frost-100 dark:hover:bg-slate-deep-800/60"
              }`}
            >
              <span className="flex items-center gap-1.5 truncate text-sm font-medium">
                {isLive(l.id) && <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" />}
                {l.label}
              </span>
              <span className={subtle}>{new Date(l.started_at).toLocaleString()}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className={subtle}>{liveRun ? "Live output" : "Saved log"}</span>
          {!liveRun && (
            <Button onClick={() => (logs.refetch(), file.refetch())}>
              <RefreshCw className="size-3.5" /> Refresh
            </Button>
          )}
        </div>
        {file.error && !liveRun && <ErrorText>{errorMessage(file.error)}</ErrorText>}
        {current && !liveRun && <DoctorPanel bottleId={bottleId} logId={current} />}
        <pre
          ref={view}
          className="min-h-0 flex-1 overflow-auto rounded-lg border border-frost-200 bg-frost-50 p-3 font-mono text-xs whitespace-pre-wrap select-text dark:border-slate-deep-700 dark:bg-slate-deep-950"
        >
          {text || "(no output)"}
        </pre>
      </div>
    </div>
    </div>
  );
}
