import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { JobsProvider } from "./api/jobs";
import { RosettaProvider } from "./api/rosetta";
import { RunsProvider } from "./api/runs";
import { App } from "./App";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <JobsProvider>
        <RunsProvider>
          <RosettaProvider>
            <App />
          </RosettaProvider>
        </RunsProvider>
      </JobsProvider>
    </QueryClientProvider>
  </StrictMode>,
);
