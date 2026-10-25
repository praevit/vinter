import { useEffect, useRef, useState, type ReactNode } from "react";
import { PackagePlus, Play, Plus, Settings, Square, Wine } from "lucide-react";
import { useAllPrograms, type AppEntry } from "../api/allPrograms";
import { commands, events } from "../api/bindings";
import { useBottles } from "../api/bottles";
import { isMac } from "../api/contextMenu";
import { errorMessage } from "../api/errors";
import { useEnsureRosetta } from "../api/rosetta";
import { runningRuns, useRuns } from "../api/runs";
import { useToasts } from "../api/toasts";
import { ProgramIcon } from "./InstallDialog";
import type { Route } from "./Sidebar";
import { subtle } from "./ui";

type Item = {
  id: string;
  label: string;
  /** Shown on the right, e.g. the bottle or "Running". */
  hint?: string;
  icon: ReactNode;
  run: () => void;
  /** Listed before typing (otherwise only when it matches). */
  suggested: boolean;
  /** Breaks ties: higher comes first. */
  rank: number;
};

/** Most items listed at once. */
const LIMIT = 50;

/**
 * Scores how well `query` matches `text`: its letters must appear in order.
 * Consecutive letters and word starts score higher. Null if it does not match.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  const t = text.toLowerCase();
  if (!q) return 0;
  let score = 0;
  let from = 0;
  let last = -2;
  for (const ch of q) {
    const at = t.indexOf(ch, from);
    if (at < 0) return null;
    if (at === last + 1) score += 3;
    if (at === 0 || /[\s\-_.(]/.test(t[at - 1])) score += 2;
    score -= Math.min(at - from, 3) * 0.1;
    last = at;
    from = at + 1;
  }
  // Shorter texts are closer matches.
  return score - t.length * 0.01;
}

/** The modifier key shortcuts use: ⌘ on macOS, Ctrl elsewhere. */
const modifier = (e: KeyboardEvent) => (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) && !e.altKey;

/**
 * ⌘K (Ctrl+K on Linux) palette: open any program, go to a bottle, or run an
 * action. Also handles the app's other keyboard shortcuts.
 */
export function CommandPalette({ route, onNavigate }: { route: Route; onNavigate: (r: Route) => void }) {
  const [open, setOpen] = useState(false);
  const { data: bottles } = useBottles();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!modifier(e) || e.shiftKey) return;
      const key = e.key.toLowerCase();
      // On macOS the app menu owns ⌘K, ⌘N and ⌘, (see `tray.rs`).
      if (!isMac && key === "k") setOpen((o) => !o);
      else if (!isMac && key === "n") onNavigate({ page: "new-bottle" });
      else if (!isMac && key === ",") onNavigate({ page: "settings" });
      else if (key === "f" && route.page === "home" && document.getElementById("home-search")) {
        document.getElementById("home-search")?.focus();
      } else if (/^[1-9]$/.test(key)) {
        // In the sidebar's order.
        const bottle = bottles?.[Number(key) - 1];
        if (!bottle) return;
        onNavigate({ page: "bottle", id: bottle.id });
      } else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    const palette = events.appNavigate.listen(({ payload }) => payload.to === "palette" && setOpen((o) => !o));
    return () => {
      window.removeEventListener("keydown", onKey);
      palette.then((u) => u());
    };
  }, [route.page, onNavigate, bottles]);

  return open ? <Palette onNavigate={onNavigate} onClose={() => setOpen(false)} /> : null;
}

function Palette({ onNavigate, onClose }: { onNavigate: (r: Route) => void; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const { data: bottles } = useBottles();
  const apps = useAllPrograms();
  const { runs, add } = useRuns();
  const ensureRosetta = useEnsureRosetta();
  const toasts = useToasts();
  const list = useRef<HTMLUListElement>(null);
  const running = runningRuns(runs);

  const go = (r: Route) => () => onNavigate(r);
  const launch = (a: AppEntry) => async () => {
    if (!(await ensureRosetta(a.needsRosetta))) return;
    try {
      add(await commands.programLaunch(a.bottle.id, a.program.index));
    } catch (e) {
      toasts.show(errorMessage(e));
    }
  };

  const items: Item[] = [
    ...apps.map((a): Item => {
      const isRunning = running.some(
        (r) => r.info.bottle_id === a.bottle.id && r.info.program_index === a.program.index,
      );
      return {
        id: `app/${a.bottle.id}/${a.program.index}`,
        label: a.program.name,
        hint: isRunning ? `${a.bottle.name} · Running` : a.bottle.name,
        icon: <ProgramIcon src={a.program.icon_data_url} size="size-5" />,
        run: launch(a),
        suggested: true,
        // Recently opened first.
        rank: a.program.last_launched ? Date.parse(a.program.last_launched) : 0,
      };
    }),
    ...(bottles ?? []).map(
      (b): Item => ({
        id: `bottle/${b.id}`,
        label: b.name,
        hint: "Bottle",
        icon: <Wine className="size-5 text-wine-500 dark:text-rose-300" />,
        run: go({ page: "bottle", id: b.id }),
        suggested: true,
        rank: -1,
      }),
    ),
    action("new-bottle", "New bottle", <Plus className="size-5" />, go({ page: "new-bottle" })),
    action("install-app", "Install an app", <PackagePlus className="size-5" />, go({ page: "install-app" })),
    action("settings", "Settings", <Settings className="size-5" />, go({ page: "settings" })),
    action("runtimes", "Runtimes", <Wine className="size-5" />, go({ page: "runtimes" })),
    ...(bottles ?? [])
      .filter((b) => b.status === "ready")
      .map(
        (b): Item => ({
          ...action(`stop/${b.id}`, `Stop all programs in ${b.name}`, <Square className="size-5" />, () =>
            void commands
              .bottleKillAll(b.id)
              .then(() => toasts.show(`Stopped every program in ${b.name}.`, "info"))
              .catch((e) => toasts.show(errorMessage(e))),
          ),
          // Before typing, only for bottles with something running.
          suggested: running.some((r) => r.info.bottle_id === b.id),
        }),
      ),
  ];

  const q = query.trim();
  const shown = (
    q
      ? items
          .map((item) => ({ item, score: fuzzyScore(q, `${item.label} ${item.hint ?? ""}`) }))
          .filter((m): m is { item: Item; score: number } => m.score !== null)
          // Matches in the label itself beat matches in the hint.
          .map((m) => ({ ...m, score: m.score + (fuzzyScore(q, m.item.label) !== null ? 5 : 0) }))
          .sort((a, b) => b.score - a.score || b.item.rank - a.item.rank)
          .map((m) => m.item)
      : items.filter((i) => i.suggested).sort((a, b) => b.rank - a.rank)
  ).slice(0, LIMIT);
  const current = Math.min(selected, Math.max(shown.length - 1, 0));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    list.current?.querySelector(`[data-index="${current}"]`)?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    onClose();
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setSelected(Math.min(current + 1, shown.length - 1));
    else if (e.key === "ArrowUp") setSelected(Math.max(current - 1, 0));
    else if (e.key === "Enter") choose(shown[current]);
    else return;
    e.preventDefault();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-slate-deep-950/40 p-6 pt-[15vh] backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="flex max-h-[60vh] w-full max-w-lg flex-col overflow-hidden rounded-2xl border border-frost-200 bg-white shadow-xl dark:border-slate-deep-700 dark:bg-slate-deep-900"
        onClick={(e) => e.stopPropagation()}
      >
        <input
          autoFocus
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={shown[current] ? `palette-${current}` : undefined}
          className="border-b border-frost-200 bg-transparent px-4 py-3 text-sm outline-none dark:border-slate-deep-700"
          placeholder="Open an app, go to a bottle, or run an action"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(0);
          }}
          onKeyDown={onKeyDown}
        />
        <ul id="palette-list" role="listbox" ref={list} className="overflow-y-auto p-1.5">
          {shown.length === 0 && <li className={`${subtle} px-3 py-2`}>Nothing matches “{q}”.</li>}
          {shown.map((item, i) => (
            <li
              key={item.id}
              id={`palette-${i}`}
              data-index={i}
              role="option"
              aria-selected={i === current}
              onMouseMove={() => i !== current && setSelected(i)}
              onClick={() => choose(item)}
              className={`flex cursor-default items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm ${
                i === current ? "bg-frost-200 dark:bg-slate-deep-800" : ""
              }`}
            >
              <span className="flex size-5 shrink-0 items-center justify-center text-frost-400">{item.icon}</span>
              <span className="min-w-0 flex-1 truncate">{item.label}</span>
              {item.hint && <span className={`${subtle} shrink-0`}>{item.hint}</span>}
              {item.id.startsWith("app/") && i === current && <Play className="size-3.5 shrink-0 text-frost-400" />}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function action(id: string, label: string, icon: ReactNode, run: () => void): Item {
  return { id: `action/${id}`, label, icon, run, suggested: true, rank: -2 };
}
