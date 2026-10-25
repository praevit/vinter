import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { commands, events } from "./api/bindings";
import { dropOnTile } from "./api/drop";
import { ApprovalDialog } from "./components/ApprovalDialog";
import { CommandPalette } from "./components/CommandPalette";
import { OpenFileDialog } from "./components/OpenFileDialog";
import { useBottles } from "./api/bottles";
import { onNavigate } from "./api/navigate";
import { isMac, useAppearance } from "./api/theme";
import { Sidebar, type Route } from "./components/Sidebar";
import { UpdateBanner } from "./components/UpdateBanner";
import { BottleView } from "./pages/BottleView";
import { About } from "./pages/About";
import { InstallApp } from "./pages/InstallApp";
import { NewBottle } from "./pages/NewBottle";
import { Runtimes } from "./pages/Runtimes";
import { Settings } from "./pages/Settings";
import { System } from "./pages/System";
import { Welcome } from "./pages/Welcome";

export function App() {
  const [route, setPage] = useState<Route>({ page: "home" });
  // macOS: Settings has its own window, like other Mac apps.
  const setRoute = useCallback((r: Route) => {
    if (isMac && r.page === "settings") void commands.settingsWindowShow();
    else setPage(r);
  }, []);
  useAppearance();
  const bottles = useBottles();
  // .exe/.msi files opened with Vinter or dropped on the window, one at a time.
  const [openFiles, setOpenFiles] = useState<string[]>([]);

  useEffect(() => {
    const take = () =>
      commands.openFilesTake().then((files) => files.length && setOpenFiles((q) => [...q, ...files]));
    void take();
    const opened = events.appOpenFiles.listen(() => void take());
    // The palette opens itself (see CommandPalette).
    const navigate = events.appNavigate.listen(({ payload }) => payload.to !== "palette" && setRoute({ page: payload.to }));
    const stopNavigating = onNavigate(setRoute);
    const showBottle = events.appShowBottle.listen(({ payload }) => setRoute({ page: "bottle", id: payload.bottle_id }));
    const dropped = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // Files dropped on a program tile open with that program instead.
      const files = dropOnTile(payload).filter((p) => /\.(exe|msi|msix|msixbundle|appx|appxbundle|vinterbottle)$/i.test(p));
      if (files.length) setOpenFiles((q) => [...q, ...files]);
    });
    return () => {
      opened.then((u) => u());
      navigate.then((u) => u());
      stopNavigating();
      showBottle.then((u) => u());
      dropped.then((u) => u());
    };
  }, [setRoute]);

  // A deleted bottle's page falls back to home.
  const current =
    route.page === "bottle" && bottles.data && !bottles.data.some((b) => b.id === route.id)
      ? { page: "home" as const }
      : route;

  return (
    <div className="flex h-screen">
      <Sidebar route={current} onNavigate={setRoute} />
      <main className="min-w-0 flex-1 overflow-y-auto vibrancy:bg-frost-50 vibrancy:dark:bg-slate-deep-950">
        {/* macOS: the window has no title bar; drag it from here. */}
        <div
          data-tauri-drag-region
          className="sticky top-0 z-10 hidden h-7 bg-frost-50 macos:block dark:bg-slate-deep-950"
        />
        <UpdateBanner route={current} onNavigate={setRoute} />
        {current.page === "bottle" ? (
          <BottleView key={current.id} id={current.id} focus={current.focus} initialTab={current.tab} onNavigate={setRoute} />
        ) : current.page === "new-bottle" ? (
          <NewBottle onNavigate={setRoute} />
        ) : current.page === "install-app" ? (
          <InstallApp onNavigate={setRoute} />
        ) : current.page === "runtimes" ? (
          <Runtimes />
        ) : current.page === "system" ? (
          <System />
        ) : current.page === "settings" ? (
          <Settings />
        ) : current.page === "about" ? (
          <About />
        ) : (
          <Welcome onNavigate={setRoute} />
        )}
      </main>
      <ApprovalDialog />
      <CommandPalette route={current} onNavigate={setRoute} />
      {openFiles.length > 0 && (
        <OpenFileDialog
          key={openFiles[0]}
          path={openFiles[0]}
          onNavigate={setRoute}
          onClose={() => setOpenFiles((q) => q.slice(1))}
        />
      )}
    </div>
  );
}
