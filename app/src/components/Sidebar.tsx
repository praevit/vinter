import { Activity, AlertTriangle, Info, LoaderCircle, PackagePlus, Plus, Wine, type LucideIcon } from "lucide-react";
import { useBottles } from "../api/bottles";

export type Route =
  | { page: "home" }
  | { page: "bottle"; id: string }
  | { page: "new-bottle" }
  | { page: "install-app" }
  | { page: "runtimes" }
  | { page: "system" }
  /** `check` starts an update check; a new value checks again. */
  | { page: "about"; check?: number };

const item = (active: boolean) =>
  `flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${
    active
      ? "bg-frost-200 font-medium dark:bg-slate-deep-800"
      : "text-slate-deep-700 hover:bg-frost-200/60 dark:text-frost-300 dark:hover:bg-slate-deep-800/60"
  }`;

export function Sidebar({ route, onNavigate }: { route: Route; onNavigate: (r: Route) => void }) {
  const { data: bottles } = useBottles();

  const NavItem = ({ to, label, Icon }: { to: Route; label: string; Icon: LucideIcon }) => (
    <button
      type="button"
      onClick={() => onNavigate(to)}
      aria-current={route.page === to.page ? "page" : undefined}
      className={item(route.page === to.page)}
    >
      <Icon className="size-4" />
      {label}
    </button>
  );

  return (
    <nav className="flex w-56 shrink-0 flex-col gap-4 border-r border-frost-200 bg-frost-100/60 p-3 dark:border-slate-deep-800 dark:bg-slate-deep-900/60">
      <button
        type="button"
        onClick={() => onNavigate({ page: "home" })}
        className="flex items-center gap-2 px-2 pt-1"
      >
        <img src="/vinter.svg" alt="" className="size-7" draggable={false} />
        <span className="font-semibold tracking-tight">Vinter</span>
      </button>

      <NavItem to={{ page: "install-app" }} label="Install an App" Icon={PackagePlus} />

      <div className="flex min-h-0 flex-1 flex-col gap-1">
        <div className="flex items-center justify-between px-2">
          <span className="text-xs font-semibold tracking-wide text-frost-400 uppercase">Bottles</span>
          <button
            type="button"
            title="New bottle"
            onClick={() => onNavigate({ page: "new-bottle" })}
            className="rounded p-0.5 text-frost-400 hover:bg-frost-200 hover:text-slate-deep-900 dark:hover:bg-slate-deep-800 dark:hover:text-frost-100"
          >
            <Plus className="size-4" />
          </button>
        </div>
        <div className="flex flex-col gap-0.5 overflow-y-auto">
          {bottles?.length === 0 && (
            <p className="px-2 py-1 text-xs text-frost-400">No bottles yet.</p>
          )}
          {bottles?.map((b) => {
            const active = route.page === "bottle" && route.id === b.id;
            return (
              <button
                key={b.id}
                type="button"
                onClick={() => onNavigate({ page: "bottle", id: b.id })}
                aria-current={active ? "page" : undefined}
                className={item(active)}
              >
                <Wine className="size-4 shrink-0 text-wine-500 dark:text-rose-300" />
                <span className="min-w-0 flex-1 truncate">{b.name}</span>
                {b.status === "setting_up" && (
                  <LoaderCircle className="size-3.5 shrink-0 animate-spin text-frost-400" />
                )}
                {b.status === "incomplete" && (
                  <AlertTriangle className="size-3.5 shrink-0 text-amber-500" />
                )}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-col gap-0.5 border-t border-frost-200 pt-3 dark:border-slate-deep-800">
        <NavItem to={{ page: "runtimes" }} label="Runtimes" Icon={Wine} />
        <NavItem to={{ page: "system" }} label="System" Icon={Activity} />
        <NavItem to={{ page: "about" }} label="About" Icon={Info} />
      </div>
    </nav>
  );
}
