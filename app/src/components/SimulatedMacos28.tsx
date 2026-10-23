import { useExperimental } from "../api/experimental";
import { ExperimentalBadge } from "./ExperimentalBadge";

/** Shown where Rosetta runtimes would appear while `simulate-macos28` hides them. */
export function SimulatedMacos28Notice() {
  if (!useExperimental("simulate-macos28")) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-xl border border-violet-300/60 bg-violet-50 p-3.5 text-sm dark:border-violet-500/30 dark:bg-violet-500/10">
      <ExperimentalBadge />
      <span>
        Simulating macOS 28: Intel (Rosetta) runtimes are hidden and their bottles cannot start. Turn off "Pretend
        Rosetta is gone" in Settings → Experimental to bring them back.
      </span>
    </div>
  );
}
