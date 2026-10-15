import { useEffect, useState } from "react";
import { events } from "./api/bindings";
import { useBottles } from "./api/bottles";
import { Sidebar, type Route } from "./components/Sidebar";
import { BottleView } from "./pages/BottleView";
import { About } from "./pages/About";
import { InstallApp } from "./pages/InstallApp";
import { NewBottle } from "./pages/NewBottle";
import { Runtimes } from "./pages/Runtimes";
import { System } from "./pages/System";
import { Welcome } from "./pages/Welcome";

export function App() {
  const [route, setRoute] = useState<Route>({ page: "home" });
  const bottles = useBottles();

  // The menu bar icon's menu can ask for a page.
  useEffect(() => {
    const unlisten = events.appNavigate.listen(({ payload }) => setRoute({ page: payload.to }));
    return () => {
      unlisten.then((u) => u());
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
        ) : current.page === "about" ? (
          <About />
        ) : (
          <Welcome onNavigate={setRoute} />
        )}
      </main>
    </div>
  );
}
