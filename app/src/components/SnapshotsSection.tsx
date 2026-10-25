import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Camera, RotateCcw, Trash2 } from "lucide-react";
import { commands, type Snapshot } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { Button, ErrorText, field, subtle } from "./ui";

/** Snapshots of a bottle, to roll back to. For a bottle's Snapshots & backups tab. */
export function SnapshotsSection({ bottleId }: { bottleId: string }) {
  const queryClient = useQueryClient();
  const { data: snapshots } = useQuery({
    queryKey: ["bottles", bottleId, "snapshots"],
    queryFn: () => commands.snapshotsList(bottleId),
  });
  const [label, setLabel] = useState("");
  // Restoring replaces the bottle's files and deleting cannot be undone, so both ask first.
  const [confirming, setConfirming] = useState<string>();
  const [confirmingDelete, setConfirmingDelete] = useState<string>();
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["bottles"] });

  const create = useMutation({
    mutationFn: () => commands.snapshotCreate(bottleId, label),
    onSuccess: () => setLabel(""),
    onSettled: refresh,
  });
  const restore = useMutation({
    mutationFn: (id: string) => commands.snapshotRestore(bottleId, id),
    onSettled: () => {
      setConfirming(undefined);
      return refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => commands.snapshotDelete(bottleId, id),
    onSettled: () => {
      setConfirmingDelete(undefined);
      return refresh();
    },
  });
  const busy = create.isPending || restore.isPending || remove.isPending;
  const error = create.error ?? restore.error ?? remove.error;

  return (
    <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">Snapshots</h2>
      <p className={subtle}>
        Saves the bottle as it is now, to roll back to later. Vinter also takes one before installing
        a program or switching runtime. On most Macs they are instant and take no space until the
        bottle changes.
      </p>
      <div className="flex gap-2">
        <input
          className={field}
          value={label}
          maxLength={64}
          placeholder="What it is for, e.g. before mods"
          onChange={(e) => setLabel(e.target.value)}
        />
        <Button disabled={busy} onClick={() => create.mutate()}>
          <Camera className="size-4" /> {create.isPending ? "Saving…" : "Take snapshot"}
        </Button>
      </div>
      {snapshots && snapshots.length > 0 && (
        <ul className="flex flex-col divide-y divide-frost-200 dark:divide-slate-deep-700">
          {snapshots.map((s) => (
            <SnapshotRow
              key={s.id}
              snapshot={s}
              busy={busy}
              confirming={confirming === s.id}
              confirmingDelete={confirmingDelete === s.id}
              restoring={restore.isPending && restore.variables === s.id}
              onRestore={() => {
                setConfirmingDelete(undefined);
                if (confirming === s.id) restore.mutate(s.id);
                else setConfirming(s.id);
              }}
              onCancel={() => {
                setConfirming(undefined);
                setConfirmingDelete(undefined);
              }}
              onDelete={() => {
                setConfirming(undefined);
                if (confirmingDelete === s.id) remove.mutate(s.id);
                else setConfirmingDelete(s.id);
              }}
            />
          ))}
        </ul>
      )}
      {error && <ErrorText>{errorMessage(error)}</ErrorText>}
    </section>
  );
}

function SnapshotRow({
  snapshot,
  busy,
  confirming,
  confirmingDelete,
  restoring,
  onRestore,
  onCancel,
  onDelete,
}: {
  snapshot: Snapshot;
  busy: boolean;
  confirming: boolean;
  confirmingDelete: boolean;
  restoring: boolean;
  onRestore: () => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  return (
    <li className="flex items-center gap-2 py-2">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm">{snapshot.label}</span>
        <span className={subtle}>
          {new Date(snapshot.created_at).toLocaleString()}
          {snapshot.automatic && " · automatic"}
        </span>
      </div>
      {confirming ? (
        <>
          <Button variant="primary" disabled={busy} onClick={onRestore}>
            <RotateCcw className="size-4" />
            {restoring ? "Restoring…" : "Stop programs and restore"}
          </Button>
          <Button disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
        </>
      ) : confirmingDelete ? (
        <>
          <Button variant="primary" disabled={busy} onClick={onDelete}>
            <Trash2 className="size-4" aria-hidden /> Delete snapshot
          </Button>
          <Button disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
        </>
      ) : (
        <>
          <Button disabled={busy} onClick={onRestore} title="Put the bottle back as it was">
            <RotateCcw className="size-4" /> Restore
          </Button>
          <Button variant="danger" disabled={busy} onClick={onDelete} title="Delete snapshot" aria-label="Delete snapshot">
            <Trash2 className="size-4" />
          </Button>
        </>
      )}
    </li>
  );
}
