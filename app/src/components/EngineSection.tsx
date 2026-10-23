import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type ProgramView } from "../api/bindings";
import { useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { useExperimental } from "../api/experimental";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { ErrorText, subtle } from "./ui";

/** Run an app's own ARM64 build instead, in Apple Silicon bottles (experimental `per-app-engine`). */
export function EngineSection({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const enabled = useExperimental("per-app-engine");
  const { data: bottles } = useBottles();
  const fex = bottles?.find((b) => b.id === bottleId)?.engine === "arm64-fex";
  const queryClient = useQueryClient();
  const { data: builds } = useQuery({
    queryKey: ["arm64-builds", bottleId, program.index],
    queryFn: () => commands.programArm64Builds(bottleId, program.index),
    enabled: enabled && fex,
  });
  const [error, setError] = useState<string>();
  if (!enabled || !fex) return null;

  const choose = async (build: string | null) => {
    setError(undefined);
    try {
      await commands.programSetArm64Build(bottleId, program.index, build);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <section className="flex flex-col gap-2 rounded-xl border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        Engine for this app <ExperimentalBadge />
      </h3>
      {!builds?.length ? (
        <p className={subtle}>No ARM64 build of this app was found next to it, so it runs as installed.</p>
      ) : (
        <>
          <label className="flex items-start gap-2 text-sm">
            <input type="radio" checked={!program.arm64_build} onChange={() => choose(null)} className="mt-1" />
            As installed (Intel code translated by FEX)
          </label>
          {builds.map((b) => (
            <label key={b} className="flex items-start gap-2 text-sm">
              <input
                type="radio"
                checked={program.arm64_build === b}
                onChange={() => choose(b)}
                className="mt-1"
              />
              <span>
                Native ARM64 build, no emulation
                <span className={`block break-all ${subtle}`}>{b}</span>
              </span>
            </label>
          ))}
        </>
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}
