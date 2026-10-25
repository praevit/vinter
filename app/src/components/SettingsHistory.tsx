import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Undo2 } from "lucide-react";
import { commands, type HistoryItem } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs } from "../api/jobs";
import { Button, ErrorText, subtle } from "./ui";

/** How many changes show before "Show all". */
const SHOWN = 5;

/** A bottle's recent settings changes, each with Revert. For a bottle's Settings tab. */
export function SettingsHistory({ bottleId }: { bottleId: string }) {
  const queryClient = useQueryClient();
  const jobs = useJobs();
  const jobKey = `bottle-revert:${bottleId}`;
  const { data: items } = useQuery({
    queryKey: ["bottles", bottleId, "history"],
    queryFn: () => commands.bottleHistory(bottleId),
  });
  const [all, setAll] = useState(false);
  const [reverting, setReverting] = useState<string>();
  const [error, setError] = useState<string>();
  const job = jobs.job(jobKey);
  const busy = reverting !== undefined || isRunning(job);

  const revert = async (item: HistoryItem) => {
    setError(undefined);
    setReverting(item.id);
    try {
      const jobId = await commands.bottleHistoryRevert(bottleId, item.id);
      if (jobId) await jobs.start(jobKey, async () => jobId);
      await queryClient.invalidateQueries({ queryKey: ["bottles", bottleId] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setReverting(undefined);
    }
  };

  if (!items || items.length === 0) return null;
  const shown = all ? items : items.slice(0, SHOWN);

  return (
    <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">History</h2>
      <p className={subtle}>
        Recent changes to this bottle's settings. Revert puts one setting back as it was; that is a change too, so
        you can undo it.
      </p>
      <ul className="flex flex-col divide-y divide-frost-200 dark:divide-slate-deep-700">
        {shown.map((item) => (
          <li key={item.id} className="flex items-center gap-2 py-2">
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm">
                {item.title}: <span className="select-text">{item.detail}</span>
              </span>
              <span className={subtle}>{new Date(item.at).toLocaleString()}</span>
            </div>
            <Button disabled={busy} onClick={() => revert(item)} title="Put this setting back as it was">
              <Undo2 className="size-4" /> {reverting === item.id ? "Reverting…" : "Revert"}
            </Button>
          </li>
        ))}
      </ul>
      {items.length > SHOWN && (
        <button type="button" className={`${subtle} self-start underline`} onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${items.length}`}
        </button>
      )}
      {isRunning(job) && <p className={subtle}>Reverting…</p>}
      {job?.done && !job.done.ok && <ErrorText>{job.done.error}</ErrorText>}
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}
