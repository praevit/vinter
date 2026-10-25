import { useState } from "react";
import { FileText, FileUp, RotateCcw } from "lucide-react";
import { commands, type RecipeState } from "../api/bindings";
import { bottleJobKey } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { useJobs } from "../api/jobs";
import { useRecipes } from "../api/recipes";
import { Button, ErrorText, subtle } from "./ui";

/** Shown in a bottle whose recipe setup stopped, to continue it. */
export function RecipeResume({
  bottleId,
  state,
  onShowLog,
}: {
  bottleId: string;
  state: RecipeState;
  /** Opens the bottle's logs. */
  onShowLog?: () => void;
}) {
  const jobs = useJobs();
  const { data } = useRecipes();
  const names = data?.recipes.find((r) => r.id === state.recipe_id)?.winetricks_names ?? [];
  const [installer, setInstaller] = useState<string>();
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState<string>();

  const resume = () =>
    jobs
      .start(bottleJobKey(bottleId), () =>
        commands.recipeResume(bottleId, installer ?? null, consent),
      )
      .catch((e) => setError(errorMessage(e)));

  return (
    <div className="flex flex-col items-start gap-3 rounded-xl border border-amber-300/60 bg-amber-50 p-4 text-sm dark:border-amber-500/30 dark:bg-amber-500/10">
      <p className="font-medium">
        Setting up {state.recipe_name} did not finish ({state.completed_steps} of {state.total_steps} steps
        done).
      </p>
      <p className={subtle}>
        Try again to continue from where it stopped. If it stops again, its log shows what went wrong.
      </p>
      {state.needs_installer && (
        <Button
          onClick={async () => {
            const picked = await commands.installerChoose();
            if (picked) setInstaller(picked);
          }}
        >
          <FileUp className="size-4" /> {installer ? installer.split(/[\\/]/).pop() : "Choose the installer again…"}
        </Button>
      )}
      {names.length > 0 && (
        <label className="flex items-start gap-2">
          <input type="checkbox" className="mt-1 accent-wine-500" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
          <span>
            I agree to Vinter downloading <strong>{names.join(", ")}</strong> (may include Microsoft software).
          </span>
        </label>
      )}
      <div className="flex gap-2">
        <Button
          variant="primary"
          disabled={(state.needs_installer && !installer) || (names.length > 0 && !consent)}
          onClick={resume}
        >
          <RotateCcw className="size-4" /> Retry
        </Button>
        {onShowLog && (
          <Button onClick={onShowLog}>
            <FileText className="size-4" /> Show log
          </Button>
        )}
      </div>
      {state.error && (
        <details className="w-full">
          <summary className="cursor-pointer text-xs text-frost-400">Details</summary>
          <pre className="mt-2 max-h-40 overflow-auto text-xs whitespace-pre-wrap select-text">{state.error}</pre>
        </details>
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
