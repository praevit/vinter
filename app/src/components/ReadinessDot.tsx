import type { Readiness, ReadinessLevel } from "../api/bindings";

const colors: Record<Exclude<ReadinessLevel, "not_applicable">, string> = {
  green: "bg-emerald-500",
  yellow: "bg-amber-400",
  red: "bg-rose-500",
};

export const READINESS_LABELS: Record<ReadinessLevel, string> = {
  green: "Ready without Rosetta",
  yellow: "Uses Rosetta",
  red: "Needs Rosetta",
  not_applicable: "",
};

/** Rosetta readiness of a bottle, with its explanation as a tooltip. Nothing where Rosetta does not apply. */
export function ReadinessDot({ readiness }: { readiness: Readiness }) {
  if (readiness.level === "not_applicable") return null;
  return (
    <span
      role="img"
      aria-label={READINESS_LABELS[readiness.level]}
      title={readiness.summary}
      className={`size-2 shrink-0 rounded-full ${colors[readiness.level]}`}
    />
  );
}
