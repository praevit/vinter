import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { JobsProvider } from "./api/jobs";
import { RosettaProvider } from "./api/rosetta";
import { RunsProvider } from "./api/runs";
import { startTheme } from "./api/theme";
import { ToastProvider } from "./api/toasts";
import { App } from "./App";
import { AboutWindow } from "./pages/AboutWindow";
import { LaunchWindow } from "./pages/LaunchWindow";
import { SettingsWindow } from "./pages/SettingsWindow";
import { UpdatesWindow } from "./pages/UpdatesWindow";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

// The same bundle serves the Settings, About, update and launch windows (see `windows.rs`).
const windowKind = (window as { __VINTER_WINDOW__?: string }).__VINTER_WINDOW__;
startTheme({ vibrancy: windowKind === undefined });
const root =
  windowKind === "launch" ? (
    <LaunchWindow />
  ) : windowKind === "about" ? (
    <AboutWindow />
  ) : windowKind === "settings" ? (
    <ToastProvider background={false}>
      <JobsProvider>
        <SettingsWindow />
      </JobsProvider>
    </ToastProvider>
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
