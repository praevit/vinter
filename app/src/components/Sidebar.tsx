import { Activity, Wine, type LucideIcon } from "lucide-react";

export type Page = "runtimes" | "system";

const ITEMS: { page: Page; label: string; Icon: LucideIcon }[] = [
  { page: "runtimes", label: "Runtimes", Icon: Wine },
  { page: "system", label: "System", Icon: Activity },
];

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (page: Page) => void }) {
  return (
    <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-frost-200 bg-frost-100/60 p-3 dark:border-slate-deep-800 dark:bg-slate-deep-900/60">
      <div className="mb-4 flex items-center gap-2 px-2 pt-1">
        <img src="/vinter.svg" alt="" className="size-7" draggable={false} />
        <span className="font-semibold tracking-tight">Vinter</span>
      </div>
      {ITEMS.map(({ page: p, label, Icon }) => (
        <button
          key={p}
          type="button"
          onClick={() => onNavigate(p)}
          aria-current={page === p ? "page" : undefined}
          className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${
            page === p
              ? "bg-frost-200 font-medium dark:bg-slate-deep-800"
              : "text-slate-deep-700 hover:bg-frost-200/60 dark:text-frost-300 dark:hover:bg-slate-deep-800/60"
          }`}
        >
          <Icon className="size-4" />
          {label}
        </button>
      ))}
    </nav>
  );
}
