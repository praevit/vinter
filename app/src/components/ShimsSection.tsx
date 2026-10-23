import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type ProgramView } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { useExperimental } from "../api/experimental";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { ErrorText, subtle } from "./ui";

/** Known fixes for game launchers (experimental `launcher-shims`). */
export function ShimsSection({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const enabled = useExperimental("launcher-shims");
  const queryClient = useQueryClient();
  const key = ["shims", bottleId, program.index];
  const { data: shims } = useQuery({
    queryKey: key,
    queryFn: () => commands.programShims(bottleId, program.index),
    enabled,
  });
  const [error, setError] = useState<string>();
  if (!enabled || !shims?.length) return null;

  const toggle = async (id: string, on: boolean) => {
    setError(undefined);
    try {
      await commands.programShimSet(bottleId, program.index, id, on);
      await queryClient.invalidateQueries({ queryKey: key });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <section className="flex flex-col gap-2 rounded-xl border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        Launcher fixes <ExperimentalBadge />
      </h3>
      {shims.map((s) => (
        <label key={s.id} className="flex items-start gap-2 text-sm">
          <input type="checkbox" className="mt-1" checked={s.on} onChange={(e) => toggle(s.id, e.target.checked)} />
          <span className="flex flex-col gap-0.5">
            <span className="font-medium">
              {s.launcher}: {s.title}
            </span>
            <span className={subtle}>{s.why}</span>
            <span className={subtle}>
              Adds <code className="select-text">{s.args.join(" ")}</code> when it starts (its saved arguments are not
              changed).{" "}
              <button type="button" className="underline" onClick={() => commands.shimOpenSource(s.id)}>
                Documented by {s.launcher}
              </button>
            </span>
          </span>
        </label>
      ))}
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}
