import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type Bottle_Serialize,
  type BottlePatch,
  type Tool,
  type WindowsVersion,
} from "./bindings";

/** A bottle as the backend sends it (all fields present). */
export type BottleDetails = Bottle_Serialize;

export const WINDOWS_VERSIONS: { value: WindowsVersion; label: string }[] = [
  { value: "win11", label: "Windows 11" },
  { value: "win10", label: "Windows 10" },
  { value: "win81", label: "Windows 8.1" },
  { value: "win8", label: "Windows 8" },
  { value: "win7", label: "Windows 7" },
  { value: "winxp64", label: "Windows XP" },
];

export const windowsLabel = (v: WindowsVersion) =>
  WINDOWS_VERSIONS.find((w) => w.value === v)?.label ?? v;

export const TOOLS: { tool: Tool; label: string; description: string }[] = [
  { tool: "notepad", label: "Notepad", description: "Wine's built-in text editor" },
  { tool: "explorer", label: "File Explorer", description: "Browse the bottle's drives" },
  { tool: "winecfg", label: "Wine Settings", description: "Drives, audio, graphics, libraries" },
  { tool: "regedit", label: "Registry Editor", description: "Edit the Windows registry" },
  { tool: "taskmgr", label: "Task Manager", description: "See and stop Windows programs" },
  { tool: "control", label: "Control Panel", description: "Add/remove programs, game controllers" },
];

export const bottleJobKey = (id: string) => `bottle:${id}`;
export const windowsJobKey = (id: string) => `bottle-windows:${id}`;

export function useBottles() {
  return useQuery({ queryKey: ["bottles"], queryFn: () => commands.bottlesList() });
}

export function useBottle(id: string) {
  return useQuery({ queryKey: ["bottles", id], queryFn: () => commands.bottleGet(id) });
}

export function useUpdateBottle(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: BottlePatch) => commands.bottleUpdate(id, patch),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bottles"] }),
  });
}

export function useDeleteBottle() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => commands.bottleDelete(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bottles"] }),
  });
}
