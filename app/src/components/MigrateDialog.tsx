import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands } from "../api/bindings";
import { bottleJobKey } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { useJobs } from "../api/jobs";
import { formatBytes } from "../api/runtimes";
import { Dialog } from "./Dialog";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { Button, ErrorText, Label, field, subtle } from "./ui";

/** "Recreate for Apple Silicon" (experimental `bottle-migrate-arm64`). */
export function MigrateDialog({
  bottleId,
  bottleName,
  onClose,
  onStarted,
}: {
  bottleId: string;
  bottleName: string;
  onClose: () => void;
  onStarted: (newBottleId: string) => void;
}) {
  const plan = useQuery({ queryKey: ["migrate-plan", bottleId], queryFn: () => commands.bottleMigratePlan(bottleId) });
  const jobs = useJobs();
  const queryClient = useQueryClient();
  const [name, setName] = useState(`${bottleName} (Apple Silicon)`);
  const [agreed, setAgreed] = useState(false);
  const [error, setError] = useState<string>();
  const p = plan.data;

  const start = async () => {
    setError(undefined);
    try {
      const started = await commands.bottleMigrateStart(bottleId, name, null, agreed);
      jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
      onStarted(started.bottle_id);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Dialog onClose={onClose}>
      <div className="flex flex-col gap-4 text-left">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          Recreate for Apple Silicon <ExperimentalBadge />
        </h2>
        {plan.error ? (
          <ErrorText>{errorMessage(plan.error)}</ErrorText>
        ) : !p ? (
          <p className={subtle}>Looking at the bottle…</p>
        ) : (
          <>
            <p className="text-sm">
              Makes a new bottle on <span className="font-medium">{p.runtime_name}</span>, which runs without Rosetta.{" "}
              {bottleName} itself is not changed.
            </p>
            <ul className="list-disc pl-5 text-sm">
              <li>
                {p.recipe
                  ? `Reinstalls its apps with the "${p.recipe}" recipe.`
                  : `Its apps are not reinstalled: install ${p.reinstall.join(", ") || "them"} again in the new bottle.`}
              </li>
              <li>
                Copies {p.folders.length ? p.folders.join(", ") : "no user folders"} ({formatBytes(p.bytes)}). Folders
                linked to your Mac (like Documents) stay linked.
              </li>
            </ul>
            <Label title="Name of the new bottle">
              <input className={field} value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
            </Label>
            {p.recipe && (
              <label className="flex items-start gap-2 text-sm">
                <input
                  type="checkbox"
                  className="mt-0.5 accent-wine-500"
                  checked={agreed}
                  onChange={(e) => setAgreed(e.target.checked)}
                />
                The recipe may download components (for example from Microsoft), under their own licenses.
              </label>
            )}
            <p className={subtle}>
              High risk: Apple Silicon Wine is new, and some apps will not work there yet. Keep the old bottle until the
              new one works for you.
            </p>
          </>
        )}
        {error && <ErrorText>{error}</ErrorText>}
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!p || !name.trim()} onClick={start}>
            Recreate
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
