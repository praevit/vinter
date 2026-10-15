import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { commands, events } from "./api/bindings";
import { OpenFileDialog } from "./components/OpenFileDialog";
import { useBottles } from "./api/bottles";
import { Sidebar, type Route } from "./components/Sidebar";
import { BottleView } from "./pages/BottleView";
import { About } from "./pages/About";
import { InstallApp } from "./pages/InstallApp";
import { NewBottle } from "./pages/NewBottle";
import { Runtimes } from "./pages/Runtimes";
import { Settings } from "./pages/Settings";
import { System } from "./pages/System";
import { Welcome } from "./pages/Welcome";

export function App() {
  const [route, setRoute] = useState<Route>({ page: "home" });
  const bottles = useBottles();
  // .exe/.msi files opened with Vinter or dropped on the window, one at a time.
  const [openFiles, setOpenFiles] = useState<string[]>([]);

  useEffect(() => {
    const take = () =>
      commands.openFilesTake().then((files) => files.length && setOpenFiles((q) => [...q, ...files]));
    void take();
    const opened = events.appOpenFiles.listen(() => void take());
    const navigate = events.appNavigate.listen(({ payload }) => setRoute({ page: payload.to }));
    const dropped = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type !== "drop") return;
      const files = payload.paths.filter((p) => /\.(exe|msi|vinterbottle)$/i.test(p));
      if (files.length) setOpenFiles((q) => [...q, ...files]);
    });
    return () => {
      opened.then((u) => u());
      navigate.then((u) => u());
      dropped.then((u) => u());
    };
  }, []);

  // A deleted bottle's page falls back to home.
  const current =
    route.page === "bottle" && bottles.data && !bottles.data.some((b) => b.id === route.id)
      ? { page: "home" as const }
      : route;

  return (
    <div className="flex h-screen">
      <Sidebar route={current} onNavigate={setRoute} />
      <main className="min-w-0 flex-1 overflow-y-auto">
        {current.page === "bottle" ? (
          <BottleView key={current.id} id={current.id} onNavigate={setRoute} />
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
