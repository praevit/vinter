import { LoaderCircle, RefreshCw } from "lucide-react";
import { errorMessage, useSystemChecks } from "../api/system";
import { CheckRow } from "../components/CheckRow";

export function Home() {
  const { data, error, isFetching, refetch } = useSystemChecks();

  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-8 px-6 py-12">
      <header className="flex items-center gap-4">
        <img src="/vinter.svg" alt="" className="size-16" draggable={false} />
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">Vinter</h1>
          <p className="text-slate-deep-700/70 dark:text-frost-300/70">
            Windows apps, poured onto your Mac.
          </p>
        </div>
      </header>

      <section className="rounded-xl border border-frost-200 bg-white dark:border-slate-deep-700 dark:bg-slate-deep-900">
        <div className="flex items-center justify-between border-b border-frost-200 px-4 py-3 dark:border-slate-deep-700">
          <h2 className="text-sm font-semibold">System check</h2>
          <button
            type="button"
            onClick={() => refetch()}
            disabled={isFetching}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-wine-500 hover:bg-frost-100 disabled:opacity-50 dark:text-rose-300 dark:hover:bg-slate-deep-800"
          >
            <RefreshCw className={`size-4 ${isFetching ? "animate-spin" : ""}`} />
            Run again
          </button>
        </div>

        {error ? (
          <p className="px-4 py-3 text-sm text-wine-500 dark:text-rose-400">
            Could not run the system check: {errorMessage(error)}
          </p>
        ) : !data ? (
          <p className="flex items-center gap-2 px-4 py-3 text-sm text-frost-400">
            <LoaderCircle className="size-4 animate-spin" /> Checking your system…
          </p>
        ) : (
          <ul className="divide-y divide-frost-200 dark:divide-slate-deep-700">
            {data.checks.map((check) => (
              <CheckRow key={check.id} check={check} />
            ))}
          </ul>
        )}
      </section>

      {data && (
        <footer className="text-xs text-frost-400 select-text">
          Vinter {data.version} · {data.os} ({data.arch}) · Data folder: {data.data_dir}
        </footer>
      )}
    </main>
  );
}
