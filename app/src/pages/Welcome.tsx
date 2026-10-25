import { useState } from "react";
import { PackagePlus, Plus, Search, Wine } from "lucide-react";
import { useBottles } from "../api/bottles";
import type { BottleListItem } from "../api/bindings";
import { usePrograms } from "../api/programs";
import { logRoute, navigate } from "../api/navigate";
import { useInstalledRuntimes } from "../api/runtimes";
import { NowRunning } from "../components/NowRunning";
import { ProgramTile } from "../components/ProgramsTab";
import { RecentApps } from "../components/RecentApps";
import type { Route } from "../components/Sidebar";
import { Button, field, subtle } from "../components/ui";

/** Home page: your apps, or the next step for new users. */
export function Welcome({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const noRuntime = runtimes?.length === 0;
  const withApps = bottles?.filter((b) => b.status === "ready" && b.program_count > 0) ?? [];
  const appCount = withApps.reduce((n, b) => n + b.program_count, 0);
  const [query, setQuery] = useState("");

  const actions = (
    <div className="flex gap-2">
      <Button variant="primary" onClick={() => onNavigate({ page: "install-app" })}>
        <PackagePlus className="size-4" /> Install an App
      </Button>
      <Button onClick={() => onNavigate({ page: "new-bottle" })}>
        <Plus className="size-4" /> New bottle
      </Button>
    </div>
  );

  if (!noRuntime && withApps.length > 0) {
    return (
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">
        <header className="flex flex-wrap items-center justify-between gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">Your apps</h1>
          {actions}
        </header>
        <NowRunning />
        {appCount > 4 && (
          <label className="relative">
            <Search className="pointer-events-none absolute top-2 left-2.5 size-4 text-frost-400" />
            <input
              className={`${field} pl-8`}
              id="home-search"
              placeholder="Search your apps"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label="Search your apps"
            />
          </label>
        )}
        {!query && <RecentApps onNavigate={onNavigate} />}
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3">
          {withApps.map((b) => (
            <BottleApps
              key={b.id}
              query={query}
              bottle={b}
              needsRosetta={runtimes?.find((r) => r.id === b.runtime)?.needs_rosetta ?? false}
              onOpen={() => onNavigate({ page: "bottle", id: b.id })}
            />
          ))}
        </ul>
        {query && <p className={subtle}>Showing apps whose name or bottle matches "{query}".</p>}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-8 text-center">
      <img src="/vinter.svg" alt="" className="size-20" draggable={false} />
      <h1 className="text-2xl font-semibold tracking-tight">Welcome to Vinter</h1>
      <p className="max-w-md text-slate-deep-700/80 dark:text-frost-300/80">
        {noRuntime
          ? "Start by installing a Wine runtime. Then create a bottle: a self-contained Windows environment for your programs."
          : bottles?.length
            ? "Pick a bottle in the sidebar, or create a new one."
            : "Create a bottle: a self-contained Windows environment for your programs."}
      </p>
      {noRuntime ? (
        <Button variant="primary" onClick={() => onNavigate({ page: "runtimes" })}>
          <Wine className="size-4" /> Install a runtime
        </Button>
      ) : (
        actions
      )}
    </div>
  );
}

/** One bottle's programs, as tiles in the home page grid. */
function BottleApps({
  bottle,
  query,
  needsRosetta,
  onOpen,
}: {
  bottle: BottleListItem;
  query: string;
  needsRosetta: boolean;
  onOpen: () => void;
}) {
  const { data: programs } = usePrograms(bottle.id);
  const q = query.trim().toLowerCase();
  const bottleMatches = bottle.name.toLowerCase().includes(q);
  return programs
    ?.filter((p) => !q || bottleMatches || p.name.toLowerCase().includes(q))
    .map((p) => (
      <ProgramTile
        key={`${p.index}-${p.exe}`}
        bottleId={bottle.id}
        program={p}
        needsRosetta={needsRosetta}
        onShowLog={(log) => navigate(logRoute(bottle.id, log))}
        bottle={{ name: bottle.name, open: onOpen }}
      />
    ));
}
