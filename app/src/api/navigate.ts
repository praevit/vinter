import type { Route } from "../components/Sidebar";

type Listener = (route: Route) => void;
const listeners = new Set<Listener>();

/**
 * Goes to a page from anywhere, including places outside the page tree
 * (toasts, run tracking). `App` listens and switches the route.
 */
export function navigate(route: Route) {
  listeners.forEach((l) => l(route));
}

export function onNavigate(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** A bottle's page, open on one of its logs (its latest without one). */
export const logRoute = (bottleId: string, log?: string): Route => ({
  page: "bottle",
  id: bottleId,
  focus: { tab: "logs", log },
});
