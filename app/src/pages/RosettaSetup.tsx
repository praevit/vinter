import { Cpu, LoaderCircle } from "lucide-react";
import { commands } from "../api/bindings";
import { isRunning, useJobs } from "../api/jobs";

const JOB_KEY = "rosetta";

/** Shown at launch on Apple Silicon Macs without Rosetta 2. */
export function RosettaSetup({ onSkip }: { onSkip: () => void }) {
  const jobs = useJobs();
  const job = jobs.job(JOB_KEY);
  const running = isRunning(job);
  const failed = job?.done && !job.done.ok;

  const install = () => jobs.start(JOB_KEY, () => commands.rosettaInstall());

  return (
    <main className="flex h-screen items-center justify-center px-6">
      <div className="flex max-w-md flex-col items-center gap-5 text-center">
        <img src="/vinter.svg" alt="" className="size-20" draggable={false} />
        <h1 className="text-2xl font-semibold tracking-tight">Vinter needs Rosetta 2</h1>
        <p className="text-slate-deep-700/80 dark:text-frost-300/80">
          Windows programs run through Wine, and the Wine builds for Mac are made for Intel
          processors. Rosetta 2 is Apple's free translator that lets them run on Apple Silicon. It
          is a one-time install and takes about a minute.
        </p>

        {running ? (
          <p className="flex items-center gap-2 text-sm">
            <LoaderCircle className="size-4 animate-spin" />
            Installing Rosetta 2… macOS may ask for your password.
          </p>
        ) : (
          <button
            type="button"
            onClick={install}
            className="flex items-center gap-2 rounded-lg bg-wine-500 px-4 py-2 font-medium text-white hover:bg-wine-600"
          >
            <Cpu className="size-4" />
            {failed ? "Try again" : "Install Rosetta"}
          </button>
        )}

        {failed && (
          <p className="text-sm text-wine-500 select-text dark:text-rose-400">
            {job?.done?.error}
          </p>
        )}

        <p className="text-xs text-frost-400">
          By installing, you agree to Apple's Rosetta 2 license. You can also install it yourself
          with <code className="select-text">softwareupdate --install-rosetta</code> in Terminal.
        </p>

        {!running && (
          <button
            type="button"
            onClick={onSkip}
            className="text-sm text-frost-400 underline-offset-2 hover:underline"
          >
            Not now
          </button>
        )}
      </div>
    </main>
  );
}
