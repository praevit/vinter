/** A thin bar; indeterminate (pulsing) when `percent` is null. */
export function ProgressBar({ percent }: { percent: number | null }) {
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent ?? undefined}
      className="h-1.5 w-full overflow-hidden rounded-full bg-frost-200 dark:bg-slate-deep-700"
    >
      {percent === null ? (
        <div className="h-full w-1/3 animate-pulse rounded-full bg-wine-500" />
      ) : (
        <progress
          value={percent}
          max={100}
          className="block h-full w-full appearance-none [&::-webkit-progress-bar]:bg-transparent [&::-webkit-progress-value]:rounded-full [&::-webkit-progress-value]:bg-wine-500 [&::-webkit-progress-value]:transition-all"
        />
      )}
    </div>
  );
}
