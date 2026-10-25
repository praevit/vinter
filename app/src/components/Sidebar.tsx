import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Activity, AlertTriangle, LoaderCircle, PackagePlus, Plus, Settings, Trash2, Wine, type LucideIcon } from "lucide-react";
import { commands, type BottleListItem } from "../api/bindings";
import { bottleJobKey, useBottles, useDeleteBottle } from "../api/bottles";
import { showContextMenu } from "../api/contextMenu";
import { errorMessage } from "../api/errors";
import { useJobs } from "../api/jobs";
import { runningRuns, useRuns } from "../api/runs";
import { useToasts } from "../api/toasts";
import { Dialog } from "./Dialog";
import { ReadinessDot } from "./ReadinessDot";
import { Button, ErrorText } from "./ui";

export type Route =
  | { page: "home" }
  | { page: "bottle"; id: string; focus?: BottleFocus; tab?: string }
  | { page: "new-bottle" }
  | { page: "install-app" }
  | { page: "runtimes" }
  | { page: "system" }
  | { page: "settings" }
  | { page: "about" };

/** Where a bottle's page opens: a log in the Logs tab, or a Settings section. */
export type BottleFocus = { tab: "logs"; log?: string } | { tab: "settings"; section?: "snapshots" };

const item = (active: boolean) =>
  `flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${
    active
      ? "bg-frost-200 font-medium dark:bg-slate-deep-800 vibrancy:bg-slate-deep-900/10 vibrancy:dark:bg-white/10"
      : "text-slate-deep-700 hover:bg-frost-200/60 dark:text-frost-300 dark:hover:bg-slate-deep-800/60 vibrancy:hover:bg-slate-deep-900/5 vibrancy:dark:hover:bg-white/5"
  }`;

export function Sidebar({ route, onNavigate }: { route: Route; onNavigate: (r: Route) => void }) {
  const { data: allBottles } = useBottles();
  const [filter, setFilter] = useState("");
  const bottles = allBottles?.filter((b) => b.name.toLowerCase().includes(filter.trim().toLowerCase()));
  const running = runningRuns(useRuns().runs);
  const bottleMenu = useBottleMenu(onNavigate);

  const NavItem = ({ to, label, Icon }: { to: Route; label: string; Icon: LucideIcon }) => (
    <button
      type="button"
      onClick={() => onNavigate(to)}
      aria-current={route.page === to.page ? "page" : undefined}
      className={item(route.page === to.page)}
    >
      <Icon className="size-4" aria-hidden />
      {label}
    </button>
  );

  return (
    <nav
      aria-label="Vinter"
      className="flex w-56 shrink-0 flex-col gap-4 border-r border-frost-200 bg-frost-100/60 p-3 dark:border-slate-deep-800 dark:bg-slate-deep-900/60 vibrancy:border-slate-deep-900/10 vibrancy:bg-transparent vibrancy:dark:border-black/40 vibrancy:dark:bg-transparent"
    >
      {/* macOS: room for the window buttons, and a place to drag the window by. */}
      <div data-tauri-drag-region className="-mx-3 -mt-3 hidden h-9 shrink-0 macos:block" />
      <button
        type="button"
        onClick={() => onNavigate({ page: "home" })}
        className="flex items-center gap-2 rounded-md px-2 pt-1 macos:-mt-4"
      >
        <img src="/vinter.svg" alt="" className="size-7" draggable={false} />
        <span className="font-semibold tracking-tight">Vinter</span>
      </button>

      <NavItem to={{ page: "install-app" }} label="Install an App" Icon={PackagePlus} />

      <div className="flex min-h-0 flex-1 flex-col gap-1">
        <div className="flex items-center justify-between px-2">
          <h2 className="text-xs font-semibold tracking-wide text-tertiary uppercase">Bottles</h2>
          <button
            type="button"
            title="New bottle"
            aria-label="New bottle"
            onClick={() => onNavigate({ page: "new-bottle" })}
            className="rounded p-0.5 text-tertiary hover:bg-frost-200 hover:text-slate-deep-900 dark:hover:bg-slate-deep-800 dark:hover:text-frost-100"
          >
            <Plus className="size-4" />
          </button>
        </div>
        {(allBottles?.length ?? 0) > 6 && (
          <input
            className="mx-1 rounded-md border border-frost-300 bg-white px-2 py-1 text-xs focus:border-wine-500 dark:border-slate-deep-700 dark:bg-slate-deep-950"
            placeholder="Filter bottles"
            aria-label="Filter bottles"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        )}
        <div className="flex flex-col gap-0.5 overflow-y-auto p-0.5">
          {allBottles?.length === 0 && <p className="px-2 py-1 text-xs text-tertiary">No bottles yet.</p>}
          {bottles?.map((b) => {
            const active = route.page === "bottle" && route.id === b.id;
            const runCount = running.filter((r) => r.info.bottle_id === b.id).length;
            return (
              <button
                key={b.id}
                type="button"
                onClick={() => onNavigate({ page: "bottle", id: b.id })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  bottleMenu.open(b);
                }}
                aria-current={active ? "page" : undefined}
                className={item(active)}
              >
                <Wine className="size-4 shrink-0 text-wine-500 dark:text-rose-300" aria-hidden />
                <span className="min-w-0 flex-1 truncate">{b.name}</span>
                {runCount > 0 && (
                  <span
                    title={runCount === 1 ? "1 program running" : `${runCount} programs running`}
                    className="flex items-center gap-1 text-[10px] text-emerald-600 tabular-nums dark:text-emerald-400"
                  >
                    <span className="size-1.5 rounded-full bg-emerald-500" />
                    {runCount > 1 && runCount}
                  </span>
                )}
                {b.status === "ready" && <ReadinessDot readiness={b.readiness} />}
                {b.status === "setting_up" && (
                  <LoaderCircle
                    className="size-3.5 shrink-0 animate-spin text-tertiary"
                    role="img"
                    aria-label="Setting up"
                  />
                )}
                {b.status === "incomplete" && (
                  <AlertTriangle
                    className="size-3.5 shrink-0 text-amber-600 dark:text-amber-400"
                    role="img"
                    aria-label="Setup did not finish"
                  />
                )}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-col gap-0.5 border-t border-frost-200 pt-3 dark:border-slate-deep-800 vibrancy:border-slate-deep-900/10 vibrancy:dark:border-white/10">
        <NavItem to={{ page: "runtimes" }} label="Runtimes" Icon={Wine} />
        <NavItem to={{ page: "system" }} label="System" Icon={Activity} />
        <NavItem to={{ page: "settings" }} label="Settings" Icon={Settings} />
      </div>
      {bottleMenu.dialog}
    </nav>
  );
}

/** The right-click menu of a bottle in the list, and its Move to Trash confirmation. */
function useBottleMenu(onNavigate: (r: Route) => void) {
  const queryClient = useQueryClient();
  const jobs = useJobs();
  const toasts = useToasts();
  const remove = useDeleteBottle();
  const [trashing, setTrashing] = useState<BottleListItem>();

  const attempt = (work: () => Promise<unknown>) => () =>
    void work().catch((e) => toasts.show(errorMessage(e)));

  const open = (b: BottleListItem) => {
    const ready = b.status === "ready";
    void showContextMenu([
      { text: "Open", action: () => onNavigate({ page: "bottle", id: b.id }) },
      { text: "Open C: Drive", enabled: ready, action: attempt(() => commands.bottleOpenDriveC(b.id)) },
      "separator",
      {
        text: "Duplicate",
        enabled: ready,
        action: attempt(async () => {
          const started = await commands.bottleDuplicate(b.id, `${b.name} copy`);
          jobs.start(bottleJobKey(started.bottle_id), async () => started.job_id);
          await queryClient.invalidateQueries({ queryKey: ["bottles"] });
          onNavigate({ page: "bottle", id: started.bottle_id });
        }),
      },
      {
        text: "Stop All Programs",
        enabled: ready,
        action: attempt(async () => {
          await commands.bottleKillAll(b.id);
          toasts.show(`Stopped every program in ${b.name}.`, "info");
        }),
      },
      "separator",
      {
        text: "Move to Trash…",
        action: () => {
          remove.reset();
          setTrashing(b);
        },
      },
    ]);
  };

  const close = () => !remove.isPending && setTrashing(undefined);
  const dialog = trashing && (
    <Dialog onClose={close}>
      <div className="flex flex-col gap-3">
        <h2 className="font-semibold">Move “{trashing.name}” to the Trash?</h2>
        <p className="text-sm">Its programs stop. You can still get it back from the Trash.</p>
        {remove.error && <ErrorText>{errorMessage(remove.error)}</ErrorText>}
        <div className="flex justify-end gap-2">
          <Button disabled={remove.isPending} onClick={close}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={remove.isPending}
            onClick={() =>
              remove.mutate(trashing.id, {
                onSuccess: () => setTrashing(undefined),
              })
            }
          >
            <Trash2 className="size-4" />
            {remove.isPending ? "Moving to Trash…" : "Move to Trash"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
  return { open, dialog };
}
