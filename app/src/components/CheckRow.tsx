import { CircleCheck, CircleMinus, CircleX, TriangleAlert } from "lucide-react";
import type { Check, CheckStatus } from "../api/bindings";

const ICONS: Record<CheckStatus, { Icon: typeof CircleCheck; className: string; label: string }> = {
  ok: { Icon: CircleCheck, className: "text-emerald-600 dark:text-emerald-400", label: "OK" },
  warning: { Icon: TriangleAlert, className: "text-amber-600 dark:text-amber-400", label: "Warning" },
  error: { Icon: CircleX, className: "text-wine-500 dark:text-rose-400", label: "Problem" },
  skipped: { Icon: CircleMinus, className: "text-frost-400", label: "Not needed" },
};

export function CheckRow({ check }: { check: Check }) {
  const { Icon, className, label } = ICONS[check.status];
  return (
    <li className="flex items-start gap-3 px-4 py-3">
      <Icon className={`mt-0.5 size-5 shrink-0 ${className}`} aria-label={label} />
      <div className="min-w-0">
        <div className="text-sm font-medium">{check.label}</div>
        <div className="text-sm text-slate-deep-700/70 dark:text-frost-300/70">{check.detail}</div>
      </div>
    </li>
  );
}
