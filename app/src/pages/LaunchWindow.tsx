/** Set by `windows.rs` for the "Opening …" window. */
type LaunchData = { name: string; icon: string | null };

/** A small card shown while a program opens from a launcher or the menu bar. */
export function LaunchWindow() {
  const data = (window as { __VINTER_LAUNCH__?: LaunchData }).__VINTER_LAUNCH__;
  // The window itself is transparent; the card has the rounded corners.
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";

  return (
    <div className="flex h-screen items-center justify-center p-1.5" data-tauri-drag-region>
      <div
        data-tauri-drag-region
        className="flex w-full items-center gap-3.5 rounded-2xl border border-frost-200/80 bg-white/95 px-4 py-3.5 shadow-xl backdrop-blur dark:border-slate-deep-700 dark:bg-slate-deep-900/95"
      >
        <img
          src={data?.icon ?? "/vinter.svg"}
          alt=""
          className="pointer-events-none size-11 shrink-0 object-contain"
          draggable={false}
        />
        <div className="pointer-events-none flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-baseline justify-between gap-2">
            <span className="truncate text-sm font-semibold">Opening {data?.name ?? "app"}</span>
            <span className="flex shrink-0 items-center gap-1 text-[11px] text-slate-deep-700/60 dark:text-frost-300/60">
              <img src="/vinter.svg" alt="" className="size-3.5" />
              Vinter
            </span>
          </div>
          <div className="relative h-1 w-full overflow-hidden rounded-full bg-frost-200 dark:bg-slate-deep-700">
            <div className="animate-launch absolute inset-y-0 w-2/5 rounded-full bg-wine-500" />
          </div>
        </div>
      </div>
    </div>
  );
}
