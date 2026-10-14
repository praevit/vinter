import { useState } from "react";
import { useSystemChecks } from "./api/system";
import { Sidebar, type Page } from "./components/Sidebar";
import { RosettaSetup } from "./pages/RosettaSetup";
import { Runtimes } from "./pages/Runtimes";
import { System } from "./pages/System";

export function App() {
  const [page, setPage] = useState<Page>("runtimes");
  const [skippedRosetta, setSkippedRosetta] = useState(false);
  const { data: status } = useSystemChecks();

  // Wine cannot run on Apple Silicon without Rosetta 2, so ask before anything else.
  const rosettaMissing = status?.checks.some((c) => c.id === "rosetta" && c.status === "error");
  if (rosettaMissing && !skippedRosetta) {
    return <RosettaSetup onSkip={() => setSkippedRosetta(true)} />;
  }

  return (
    <div className="flex h-screen">
      <Sidebar page={page} onNavigate={setPage} />
      <main className="min-w-0 flex-1 overflow-y-auto">
        {page === "runtimes" ? <Runtimes /> : <System />}
      </main>
    </div>
  );
}
