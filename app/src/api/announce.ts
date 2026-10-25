let region: HTMLElement | undefined;

/**
 * Reads a short message out to screen readers (politely, after what they are
 * saying), e.g. that a long job finished. Nothing is shown on screen.
 */
export function announce(message: string) {
  if (!region) {
    region = document.createElement("div");
    region.setAttribute("role", "status");
    region.setAttribute("aria-live", "polite");
    region.className = "sr-only";
    document.body.append(region);
  }
  const el = region;
  // Cleared first, so the same message twice is read twice.
  el.textContent = "";
  setTimeout(() => (el.textContent = message), 100);
}
