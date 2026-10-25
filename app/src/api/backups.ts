import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";

/** Automatic backup settings and each bottle's last automatic backup. */
export function useAutoBackups() {
  return useQuery({
    queryKey: ["auto-backup"],
    queryFn: () => commands.autoBackupGet(),
    // Backups finish in the background.
    refetchInterval: 60_000,
  });
}

/** When a backup was made, or "Never". */
export function lastBackupLabel(last: string | null | undefined): string {
  return last ? new Date(last).toLocaleString() : "Never";
}
