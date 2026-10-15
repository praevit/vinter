import { commands, type AppLink } from "../api/bindings";
import { useAppInfo } from "../api/app";
import { subtle } from "../components/ui";

/** The small About window opened from the menu bar, like a Mac app's About panel. */
export function AboutWindow() {
  const { data: info } = useAppInfo();
  const open = (link: AppLink) => () => void commands.appOpenLink(link);
  const link = "text-wine-500 hover:underline dark:text-rose-300";

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen flex-col items-center justify-center gap-1 px-6 pt-6 pb-5 text-center"
    >
      <img src="/vinter.svg" alt="" className="pointer-events-none mb-2 size-20" draggable={false} />
      <h1 className="pointer-events-none text-lg font-semibold tracking-tight">Vinter</h1>
      <p className={`pointer-events-none ${subtle}`}>Windows apps, poured onto your Mac.</p>
      <p className="pointer-events-none mt-2 text-xs select-text">Version {info?.version}</p>
      <p className="mt-3 flex gap-3 text-xs">
        <button type="button" className={link} onClick={open("website")}>
          Website
        </button>
        <button type="button" className={link} onClick={open("source")}>
          Source code
        </button>
        <button type="button" className={link} onClick={open("support")}>
          Support Vinter
        </button>
      </p>
      <p className={`pointer-events-none mt-3 ${subtle}`}>
        © 2026 Prævit ehf. Free software under the GPL.
        <br />
        Runs Windows programs with Wine.
      </p>
    </div>
  );
}
