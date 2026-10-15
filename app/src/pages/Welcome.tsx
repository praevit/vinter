import { PackagePlus, Plus, Wine } from "lucide-react";
import { useBottles } from "../api/bottles";
import { useInstalledRuntimes } from "../api/runtimes";
import type { Route } from "../components/Sidebar";
import { Button } from "../components/ui";

/** Home page: points new users to the next step. */
export function Welcome({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const noRuntime = runtimes?.length === 0;

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
        <div className="flex gap-2">
          <Button variant="primary" onClick={() => onNavigate({ page: "install-app" })}>
            <PackagePlus className="size-4" /> Install an App
          </Button>
          <Button onClick={() => onNavigate({ page: "new-bottle" })}>
            <Plus className="size-4" /> New bottle
          </Button>
        </div>
      )}
    </div>
  );
}
