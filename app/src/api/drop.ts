import { useEffect, useRef, type RefObject } from "react";
import type { DragDropEvent } from "@tauri-apps/api/webview";

/** Sent to a program tile (`data-program-tile`) when files are dropped on it; `detail` is the paths. */
export const OPEN_FILES_EVENT = "vinter:open-files";

/** The program tile under a drag position (Tauri gives physical pixels). */
function tileAt(position: { x: number; y: number }): HTMLElement | null {
  const scale = window.devicePixelRatio || 1;
  const element = document.elementFromPoint(position.x / scale, position.y / scale);
  return element?.closest<HTMLElement>("[data-program-tile]") ?? null;
}

let highlighted: HTMLElement | null = null;

function highlight(tile: HTMLElement | null) {
  if (highlighted === tile) return;
  highlighted?.removeAttribute("data-drop-over");
  tile?.setAttribute("data-drop-over", "");
  highlighted = tile;
}

/**
 * Handles files dragged over the window: a program tile under the pointer
 * lights up and gets the files when they're dropped. Returns the dropped
 * paths if no tile took them.
 */
export function dropOnTile(event: DragDropEvent): string[] {
  if (event.type === "enter" || event.type === "over") {
    highlight(tileAt(event.position));
    return [];
  }
  highlight(null);
  if (event.type !== "drop") return [];
  const tile = tileAt(event.position);
  if (!tile) return event.paths;
  tile.dispatchEvent(new CustomEvent(OPEN_FILES_EVENT, { detail: event.paths }));
  return [];
}

/** Calls `onFiles` with the files dropped on the element (a program tile, marked `data-program-tile`). */
export function useFileDrop(ref: RefObject<HTMLElement | null>, onFiles: (paths: string[]) => void) {
  const handler = useRef(onFiles);
  useEffect(() => {
    handler.current = onFiles;
  });
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const listener = (e: Event) => handler.current((e as CustomEvent<string[]>).detail);
    element.addEventListener(OPEN_FILES_EVENT, listener);
    return () => element.removeEventListener(OPEN_FILES_EVENT, listener);
  }, [ref]);
}
