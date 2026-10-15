import { useEffect, useState } from "react";

/** Set by `windows.rs` for the "Opening …" window. */
type LaunchData = { name: string; icon: string | null };

/** What the window says as the wait goes on (seconds → text). */
const STAGES: [number, (name: string) => string][] = [
  [0, () => "Waking up Windows…"],
  [2.5, (name) => `Opening ${name}…`],
  [7, () => "Almost there…"],
  [15, () => "Still working. The first start takes longest."],
];

/**
 * Shown while a program opens from a launcher or the menu bar. On macOS the
 * window is frosted glass (see `windows.rs`), so this draws no background.
 */
export function LaunchWindow() {
  const data = (window as { __VINTER_LAUNCH__?: LaunchData }).__VINTER_LAUNCH__;
  const name = data?.name ?? "your app";
  const [seconds, setSeconds] = useState(0);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const start = Date.now();
    const timer = setInterval(() => setSeconds((Date.now() - start) / 1000), 250);
    return () => clearInterval(timer);
  }, []);

  const message = [...STAGES].reverse().find(([at]) => seconds >= at)?.[1](name);

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen items-center gap-4 px-5 text-white select-none"
    >
      <div className="pointer-events-none relative size-14 shrink-0">
        {/* A gradient sweeping around a fixed rounded border. */}
        <div className="absolute -inset-[2px] overflow-hidden rounded-[18px]">
          <div className="launch-ring absolute -inset-1/2" />
        </div>
        <div className="absolute inset-0 flex items-center justify-center rounded-2xl bg-slate-deep-900 ring-1 ring-white/10">
          <img
            src={data?.icon ?? "/vinter.svg"}
            alt=""
            className="launch-breathe size-10 object-contain"
            draggable={false}
          />
        </div>
      </div>

      <div className="pointer-events-none flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-center justify-between gap-3">
          <span className="truncate text-[15px] font-semibold tracking-tight">{name}</span>
          <span className="flex shrink-0 items-center gap-1 text-[11px] font-medium text-white/50">
            <img src="/vinter.svg" alt="" className="size-3.5 opacity-80" />
            Vinter
          </span>
        </div>
        <span key={message} className="launch-fade truncate text-xs text-white/65">
          {message}
        </span>
        <div className="relative h-[3px] w-full overflow-hidden rounded-full bg-white/10">
          <div className="launch-bar absolute inset-y-0 w-1/2 rounded-full" />
        </div>
      </div>
    </div>
  );
}
