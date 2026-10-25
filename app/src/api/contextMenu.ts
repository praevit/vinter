import { Menu } from "@tauri-apps/api/menu";

export type MenuEntry = { text: string; action: () => void; enabled?: boolean } | "separator";

/** The last menu shown; freed when the next one opens. */
let previous: Menu | undefined;

/** Shows a native right-click menu at the pointer. */
export async function showContextMenu(entries: MenuEntry[]) {
  const menu = await Menu.new({
    items: entries.map((e) =>
      e === "separator" ? { item: "Separator" as const } : { text: e.text, action: e.action, enabled: e.enabled ?? true },
    ),
  });
  void previous?.close().catch(() => {});
  previous = menu;
  await menu.popup();
}

/** True on macOS, where shortcuts use ⌘ instead of Ctrl. */
export const isMac = navigator.userAgent.includes("Mac");
