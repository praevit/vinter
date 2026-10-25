import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { CircleCheck, LoaderCircle, Play } from "lucide-react";
import { useAppInfo } from "../api/app";
import { commands } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { usePrograms } from "../api/programs";
import { useEnsureRosetta } from "../api/rosetta";
import { useRuns } from "../api/runs";
import { ProgramIcon } from "./InstallDialog";
import { Button, ErrorText, subtle } from "./ui";

/**
 * The end of installing an app: "{App} is ready", with buttons to open it
 * and keep it in Launchpad and Spotlight (the app menu on Linux).
 * `index` is the program to show; the bottle's first program if not given.
 */
export function AppReady({
  bottleId,
  index,
  needsRosetta,
  onDone,
}: {
  bottleId: string;
  index?: number;
  needsRosetta: boolean;
  onDone: () => void;
}) {
  const { data: programs } = usePrograms(bottleId);
  const { data: app } = useAppInfo();
  const { add } = useRuns();
  const ensureRosetta = useEnsureRosetta();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string>();
  const [starting, setStarting] = useState(false);
  const program = programs?.find((p) => p.index === index) ?? programs?.[0];

  if (!programs) {
    return (
      <p className="flex items-center gap-2 py-4 text-sm">
        <LoaderCircle className="size-4 animate-spin" /> Finishing…
      </p>
    );
  }

  const open = async () => {
    if (!program) return;
    setError(undefined);
    if (!(await ensureRosetta(needsRosetta))) return;
    setStarting(true);
    try {
      add(await commands.programLaunch(bottleId, program.index));
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setStarting(false);
    }
  };

  const setPinned = async (pinned: boolean) => {
    if (!program) return;
    setError(undefined);
    try {
      await commands.programSetPinned(bottleId, program.index, pinned);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-col items-center gap-3 py-2 text-center">
      {program ? (
        <ProgramIcon src={program.icon_data_url} size="size-16" />
      ) : (
        <CircleCheck className="size-12 text-wine-500" />
      )}
      <h2 className="text-lg font-semibold">{program ? `${program.name} is ready` : "All set"}</h2>
      {program ? (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="accent-wine-500"
            checked={program.pinned}
            onChange={(e) => void setPinned(e.target.checked)}
          />
          {app?.os === "macos" ? "Keep in Launchpad and Spotlight" : "Keep in the app menu"}
        </label>
      ) : (
        <p className={subtle}>Find it in its bottle in the sidebar.</p>
      )}
      {error && <ErrorText>{error}</ErrorText>}
      <div className="mt-2 flex gap-2">
        <Button onClick={onDone}>Done</Button>
        {program && (
          <Button variant="primary" disabled={starting} onClick={open}>
            {starting ? <LoaderCircle className="size-4 animate-spin" /> : <Play className="size-4" />}
            Open {program.name}
          </Button>
        )}
      </div>
    </div>
  );
}
