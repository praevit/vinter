import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { JobsProvider } from "./api/jobs";
import { RosettaProvider } from "./api/rosetta";
import { RunsProvider } from "./api/runs";
import { ToastProvider } from "./api/toasts";
import { App } from "./App";
import { AboutWindow } from "./pages/AboutWindow";
import { UpdatesWindow } from "./pages/UpdatesWindow";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

// The same bundle serves the small About and update windows (see `windows.rs`).
const windowKind = (window as { __VINTER_WINDOW__?: string }).__VINTER_WINDOW__;
const root =
  windowKind === "about" ? (
    <AboutWindow />
  ) : windowKind === "updates" ? (
    <JobsProvider>
      <UpdatesWindow />
    </JobsProvider>
  ) : (
    <ToastProvider>
      <JobsProvider>
        <RunsProvider>
          <RosettaProvider>
            <App />
          </RosettaProvider>
        </RunsProvider>
      </JobsProvider>
    </ToastProvider>
  );

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>{root}</QueryClientProvider>
  </StrictMode>,
);
