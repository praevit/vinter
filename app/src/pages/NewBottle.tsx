import { useState, type FormEvent } from "react";
import { commands, type WindowsVersion } from "../api/bindings";
import { WINDOWS_VERSIONS, bottleJobKey } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { useJobs } from "../api/jobs";
import { useEnsureRosetta } from "../api/rosetta";
import { useInstalledRuntimes } from "../api/runtimes";
import type { Route } from "../components/Sidebar";
import { Button, ErrorText, Label, field } from "../components/ui";
import { useQueryClient } from "@tanstack/react-query";

export function NewBottle({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const { data: runtimes } = useInstalledRuntimes();
  const jobs = useJobs();
  const ensureRosetta = useEnsureRosetta();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [runtimeId, setRuntimeId] = useState<string>();
  const [windows, setWindows] = useState<WindowsVersion>("win10");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const runtime = runtimes?.find((r) => r.id === runtimeId) ?? runtimes?.[0];

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!runtime) return;
    setError(undefined);
    if (!(await ensureRosetta(runtime.needs_rosetta))) return;
    setBusy(true);
    try {
      const started = await commands.bottleCreate(name, runtime.id, windows);
      // Track the setup job under the bottle's id so its page shows progress.
      jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
      await queryClient.invalidateQueries({ queryKey: ["bottles"] });
      onNavigate({ page: "bottle", id: started.bottle_id });
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  if (runtimes?.length === 0) {
    return (
      <div className="mx-auto flex max-w-lg flex-col gap-4 px-8 py-10">
        <h1 className="text-2xl font-semibold tracking-tight">New bottle</h1>
        <p className="text-slate-deep-700/80 dark:text-frost-300/80">
          A bottle needs a Wine runtime, and none is installed yet.
        </p>
        <Button variant="primary" className="self-start" onClick={() => onNavigate({ page: "runtimes" })}>
          Install a runtime
        </Button>
      </div>
    );
  }

  return (
    <form onSubmit={submit} className="mx-auto flex max-w-lg flex-col gap-5 px-8 py-10">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">New bottle</h1>
        <p className="mt-1 text-sm text-slate-deep-700/70 dark:text-frost-300/70">
          A bottle is a self-contained Windows environment with its own C: drive, settings and
          programs. Use one per app, or group related apps together.
        </p>
      </header>

      <Label title="Name">
        <input
          className={field}
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. Office, Games, Notepad++"
          maxLength={64}
          autoFocus
          required
        />
      </Label>

      <Label title="Runtime">
        <select
          className={field}
          value={runtime?.id ?? ""}
          onChange={(e) => setRuntimeId(e.target.value)}
        >
          {runtimes?.map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </select>
      </Label>

      <Label title="Windows version">
        <select
          className={field}
          value={windows}
          onChange={(e) => setWindows(e.target.value as WindowsVersion)}
        >
          {WINDOWS_VERSIONS.map((v) => (
            <option key={v.value} value={v.value}>
              {v.label}
            </option>
          ))}
        </select>
      </Label>

      {error && <ErrorText>{error}</ErrorText>}

      <div className="flex gap-2">
        <Button type="submit" variant="primary" disabled={busy || !name.trim() || !runtime}>
          Create bottle
        </Button>
        <Button onClick={() => onNavigate({ page: "home" })}>Cancel</Button>
      </div>
    </form>
  );
}
