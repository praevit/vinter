import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { commands, type Comparison, type ProgramView, type Trial } from "../api/bindings";
import { useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { ENGINE_LABELS } from "../api/runtimes";
import { Dialog } from "./Dialog";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { Select } from "./Select";
import { Button, ErrorText, subtle } from "./ui";

/** Runs a program here and in another bottle, and shows both side by side (experimental `engine-ab-compare`). */
export function CompareDialog({
  bottleId,
  program,
  onClose,
}: {
  bottleId: string;
  program: ProgramView;
  onClose: () => void;
}) {
  const { data: bottles } = useBottles();
  const others = bottles?.filter((b) => b.id !== bottleId && b.status === "ready") ?? [];
  const [otherId, setOtherId] = useState<string>();
  const other = otherId ?? others.find((b) => b.engine !== bottles?.find((x) => x.id === bottleId)?.engine)?.id ?? others[0]?.id;
  const { data: otherPrograms } = useQuery({
    queryKey: ["bottles", other, "programs"],
    queryFn: () => commands.programsList(other!),
    enabled: !!other,
  });
  const [otherIndex, setOtherIndex] = useState<number>();
  const match = otherIndex ?? otherPrograms?.find((p) => p.name === program.name)?.index ?? otherPrograms?.[0]?.index;
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<Comparison>();
  const [error, setError] = useState<string>();

  const run = async () => {
    if (!other || match === undefined) return;
    setRunning(true);
    setError(undefined);
    try {
      setResult(await commands.compareRun(bottleId, program.index, other, match, 20));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog onClose={running ? undefined : onClose}>
      <div className="flex flex-col gap-4 text-left">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          Compare engines <ExperimentalBadge />
        </h2>
        <p className={subtle}>
          Starts {program.name} here, then the program below in the other bottle, for up to 20 seconds each, and
          records when a window appeared and whether it crashed. Other programs in both bottles are closed.
        </p>
        {others.length === 0 ? (
          <p className="text-sm">Make another bottle with this app first, ideally on the other engine.</p>
        ) : (
          <>
            <Select
              value={other ?? ""}
              options={others.map((b) => ({ value: b.id, label: `${b.name} (${ENGINE_LABELS[b.engine]})` }))}
              onChange={(id) => {
                setOtherId(id);
                setOtherIndex(undefined);
              }}
            />
            <Select
              value={String(match ?? "")}
              options={(otherPrograms ?? []).map((p) => ({ value: String(p.index), label: p.name }))}
              onChange={(v) => setOtherIndex(Number(v))}
            />
          </>
        )}
        {result && <Results result={result} />}
        {error && <ErrorText>{error}</ErrorText>}
        <div className="flex justify-end gap-2">
          <Button onClick={onClose} disabled={running}>
            Close
          </Button>
          <Button variant="primary" disabled={running || !other || match === undefined} onClick={run}>
            {running ? <LoaderCircle className="size-4 animate-spin" /> : null}
            {running ? "Running…" : "Start"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function Results({ result }: { result: Comparison }) {
  const rows: [string, (t: Trial) => string][] = [
    ["Bottle", (t) => t.bottle],
    ["Engine", (t) => ENGINE_LABELS[t.engine]],
    ["First window", (t) => (t.seconds_to_window === null ? "none seen" : `${t.seconds_to_window.toFixed(1)} s`)],
    ["Crashed", (t) => (t.crashed ? "yes" : "no")],
    ["Exited by itself", (t) => (t.exited ? `yes${t.exit_code === null ? "" : ` (code ${t.exit_code})`}` : "no")],
    ["FPS", (t) => (t.fps === null ? "not reported" : t.fps.toFixed(0))],
  ];
  return (
    <table className="w-full text-sm">
      <tbody>
        {rows.map(([label, f]) => (
          <tr key={label} className="border-t border-frost-200 dark:border-slate-deep-700">
            <th className={`py-1.5 pr-3 text-left font-normal ${subtle}`}>{label}</th>
            <td className="py-1.5 pr-3">{f(result.a)}</td>
            <td className="py-1.5">{f(result.b)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
