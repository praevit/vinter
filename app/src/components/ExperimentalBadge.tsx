import { FlaskConical } from "lucide-react";
import type { Risk } from "../api/bindings";

const pill = "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium leading-none";

/** Marks a feature that is behind an experimental flag. Use it wherever one shows up. */
export function ExperimentalBadge({ className = "" }: { className?: string }) {
  return (
    <span
      className={`${pill} bg-violet-100 text-violet-800 dark:bg-violet-500/15 dark:text-violet-300 ${className}`}
      title="Experimental: turned on in Settings, and may be rough"
    >
      <FlaskConical className="size-3" aria-hidden />
      Experimental
    </span>
  );
}

const riskStyles: Record<Risk, string> = {
  low: "bg-frost-100 text-slate-deep-700 dark:bg-slate-deep-800 dark:text-frost-300",
  medium: "bg-amber-100 text-amber-800 dark:bg-amber-500/15 dark:text-amber-300",
  high: "bg-rose-100 text-rose-800 dark:bg-rose-500/15 dark:text-rose-300",
};

/** How much can go wrong with an experimental feature on. */
export function RiskBadge({ risk }: { risk: Risk }) {
  return <span className={`${pill} ${riskStyles[risk]}`}>{risk[0].toUpperCase() + risk.slice(1)} risk</span>;
}
