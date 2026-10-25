import { useAppearance } from "../api/theme";
import { Settings } from "./Settings";

/** Settings in its own window (macOS, ⌘,), like a Mac app's. */
export function SettingsWindow() {
  useAppearance();
  return (
    <div className="flex h-screen flex-col">
      {/* Room for the window buttons, and a place to drag the window by. */}
      <div data-tauri-drag-region className="hidden h-7 shrink-0 macos:block" />
      <main className="min-h-0 flex-1 overflow-y-auto">
        <Settings />
      </main>
    </div>
  );
}
