import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, PackagePlus } from "lucide-react";
import { commands, type Category } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { isRunning, useJobs, type JobState } from "../api/jobs";
import { ProgressBar } from "./ProgressBar";
import { Button, ErrorText, subtle } from "./ui";

export const componentsJobKey = (bottleId: string) => `components:${bottleId}`;

export const CONSENT =
  "Winetricks downloads these from Microsoft and others, under their own licenses.";

const CATEGORIES: { category: Category; label: string }[] = [
  { category: "runtimes", label: "Visual C++ and Visual Basic" },
  { category: "dotnet", label: ".NET" },
  { category: "directx", label: "DirectX" },
  { category: "fonts", label: "Fonts" },
  { category: "other", label: "Other" },
];

/** Starts installing components; resolves once the job has finished. */
export function useInstallComponents(bottleId: string) {
  const jobs = useJobs();
  const key = componentsJobKey(bottleId);
  const [error, setError] = useState<string>();
  const install = async (verbs: string[]) => {
    setError(undefined);
    try {
      await jobs.start(key, () => commands.componentsInstall(bottleId, verbs, true));
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  return { install, job: jobs.job(key), error };
}

/** Pick components (Visual C++, .NET, fonts…) and install them with winetricks. */
export function ComponentsSection({ bottleId }: { bottleId: string }) {
  const { data: list } = useQuery({
    queryKey: ["bottles", bottleId, "components"],
    queryFn: () => commands.componentsList(bottleId),
  });
  const { install, job, error } = useInstallComponents(bottleId);
  const [picked, setPicked] = useState<string[]>([]);
  const [agreed, setAgreed] = useState(false);
  const running = isRunning(job);

  const toggle = (verb: string) =>
    setPicked((p) => (p.includes(verb) ? p.filter((v) => v !== verb) : [...p, verb]));

  return (
    <section className="flex flex-col gap-3 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <div>
        <h2 className="text-sm font-medium">Components</h2>
        <p className={subtle}>
          Windows parts many programs expect. The log doctor suggests which one a program is missing.
        </p>
      </div>
      {CATEGORIES.map(({ category, label }) => {
        const items = list?.filter((c) => c.category === category) ?? [];
        if (items.length === 0) return null;
        return (
          <div key={category} className="flex flex-col gap-1">
            <h3 className={`${subtle} font-medium uppercase`}>{label}</h3>
            <ul className="grid grid-cols-1 gap-1 sm:grid-cols-2">
              {items.map((c) => (
                <li key={c.verb}>
                  <label className="flex items-start gap-2 rounded-md p-1.5 hover:bg-frost-100 dark:hover:bg-slate-deep-800">
                    {c.installed ? (
                      <Check className="mt-0.5 size-4 shrink-0 text-emerald-600" aria-label="Installed" />
                    ) : (
                      <input
                        type="checkbox"
                        className="mt-1"
                        disabled={running}
                        checked={picked.includes(c.verb)}
                        onChange={() => toggle(c.verb)}
                      />
                    )}
                    <span className="flex flex-col">
                      <span className="text-sm">{c.name}</span>
                      <span className={subtle}>{c.installed ? "Installed" : c.description}</span>
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
      {picked.length > 0 && (
        <label className="flex items-start gap-2 text-sm">
          <input type="checkbox" className="mt-1" checked={agreed} onChange={(e) => setAgreed(e.target.checked)} />
          <span>{CONSENT} I agree.</span>
        </label>
      )}
      <div className="flex items-center gap-2">
        <Button
          variant="primary"
          disabled={running || picked.length === 0 || !agreed}
          onClick={async () => {
            await install(picked);
            setPicked([]);
            setAgreed(false);
          }}
        >
          <PackagePlus className="size-4" /> Install{picked.length > 0 && ` ${picked.length}`}
        </Button>
      </div>
      <JobProgress job={job} />
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}

export function JobProgress({ job }: { job: JobState | undefined }) {
  if (!job) return null;
  if (isRunning(job))
    return (
      <div className="flex flex-col gap-1">
        <ProgressBar percent={job.percent ?? null} />
        <p className={subtle}>
          {job.step}
          {job.message && ` · ${job.message}`} — .NET can take ten minutes or more.
        </p>
      </div>
    );
  return job.done?.ok ? <p className={subtle}>Installed.</p> : <ErrorText>{job.done?.error}</ErrorText>;
}
