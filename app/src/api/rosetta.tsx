import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Cpu, LoaderCircle } from "lucide-react";
import { commands } from "./bindings";
import { isRunning, useJobs } from "./jobs";
import { useSystemChecks } from "./system";
import { Dialog } from "../components/Dialog";

type Ensure = (needsRosetta: boolean, options?: { optional?: boolean }) => Promise<boolean>;

const RosettaContext = createContext<Ensure | null>(null);
const JOB_KEY = "rosetta";

/**
 * Asks for Rosetta 2 only when an Intel (x86_64) runtime is about to be used
 * on Apple Silicon. `ensure` resolves true when it is fine to continue:
 * Rosetta was already there, got installed, or was `optional` and skipped.
 */
export function RosettaProvider({ children }: { children: ReactNode }) {
  const { data: status, refetch } = useSystemChecks();
  const jobs = useJobs();
  const [open, setOpen] = useState<{ optional: boolean } | null>(null);
  const resolver = useRef<(ok: boolean) => void>(undefined);

  const rosettaMissing =
    status?.checks.some((c) => c.id === "rosetta" && c.status !== "ok" && c.status !== "skipped") ??
    false;

  const ensure: Ensure = useCallback(
    async (needsRosetta, options) => {
      if (!needsRosetta) return true;
      const fresh = (await refetch()).data;
      const missing = fresh?.checks.some(
        (c) => c.id === "rosetta" && c.status !== "ok" && c.status !== "skipped",
      );
      if (!missing) return true;
      setOpen({ optional: options?.optional ?? false });
      return new Promise<boolean>((resolve) => (resolver.current = resolve));
    },
    [refetch],
  );

  const finish = useCallback((ok: boolean) => {
    setOpen(null);
    resolver.current?.(ok);
    resolver.current = undefined;
  }, []);

  const job = jobs.job(JOB_KEY);
  const running = isRunning(job);
  const failed = job?.done && !job.done.ok;
  const installed = job?.done?.ok ?? false;

  // Continue once the install finished and the refreshed system check sees Rosetta.
  useEffect(() => {
    if (open && installed && !rosettaMissing) finish(true);
  }, [open, installed, rosettaMissing, finish]);

  return (
    <RosettaContext value={ensure}>
      {children}
      {open && (
        <Dialog onClose={running ? undefined : () => finish(open.optional)}>
          <div className="flex flex-col items-center gap-4 text-center">
            <Cpu className="size-10 text-wine-500" />
            <h2 className="text-xl font-semibold tracking-tight">This runtime needs Rosetta 2</h2>
            <p className="text-sm text-slate-deep-700/80 dark:text-frost-300/80">
              The Wine builds for Mac are made for Intel processors. Rosetta 2 is Apple's free
              translator that lets them run on Apple Silicon. It is a one-time install and takes
              about a minute.
            </p>
            {running ? (
              <p className="flex items-center gap-2 text-sm">
                <LoaderCircle className="size-4 animate-spin" />
                Installing Rosetta 2… macOS may ask for your password.
              </p>
            ) : (
              <button
                type="button"
                onClick={() => jobs.start(JOB_KEY, () => commands.rosettaInstall())}
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
              By installing, you agree to Apple's Rosetta 2 license. You can also run{" "}
              <code className="select-text">softwareupdate --install-rosetta</code> in Terminal.
            </p>
            {!running && (
              <button
                type="button"
                onClick={() => finish(open.optional)}
                className="text-sm text-frost-400 underline-offset-2 hover:underline"
              >
                {open.optional ? "Not now, continue" : "Not now"}
              </button>
            )}
          </div>
        </Dialog>
      )}
    </RosettaContext>
  );
}

export function useEnsureRosetta(): Ensure {
  const ensure = useContext(RosettaContext);
  if (!ensure) throw new Error("useEnsureRosetta must be used inside <RosettaProvider>");
  return ensure;
}
