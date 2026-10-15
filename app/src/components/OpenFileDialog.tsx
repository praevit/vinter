import { useState } from "react";
import { FileUp, Plus } from "lucide-react";
import { useBottles } from "../api/bottles";
import { useInstalledRuntimes } from "../api/runtimes";
import { Dialog } from "./Dialog";
import { RestoreButton } from "./BackupControls";
import { InstallDialog } from "./InstallDialog";
import { Select } from "./Select";
import type { Route } from "./Sidebar";
import { Button, Label, subtle } from "./ui";

/**
 * A .exe or .msi opened with Vinter (Finder, Open With, drag and drop):
 * asks which bottle to run it in, then continues like "Install a program".
 */
export function OpenFileDialog({
  path,
  onNavigate,
  onClose,
}: {
  path: string;
  onNavigate: (r: Route) => void;
  onClose: () => void;
}) {
  const { data: bottles } = useBottles();
  const { data: runtimes } = useInstalledRuntimes();
  const ready = bottles?.filter((b) => b.status === "ready") ?? [];
  const [bottleId, setBottleId] = useState<string>();
  const [confirmed, setConfirmed] = useState(false);
  const chosen = ready.find((b) => b.id === bottleId) ?? ready[0];
  const fileName = path.split(/[\\/]/).pop();

  if (/\.vinterbottle$/i.test(path)) {
    return (
      <Dialog onClose={onClose}>
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Restore {fileName}?</h2>
          <p className={subtle}>The backup becomes a new bottle. Your other bottles are not changed.</p>
          <RestoreButton path={path} />
          <div className="flex justify-end">
            <Button onClick={onClose}>Close</Button>
          </div>
        </div>
      </Dialog>
    );
  }

  if (confirmed && chosen) {
    return (
      <InstallDialog
        bottleId={chosen.id}
        needsRosetta={runtimes?.find((r) => r.id === chosen.runtime)?.needs_rosetta ?? false}
        start="choose"
        initialPath={path}
        onClose={() => {
          onNavigate({ page: "bottle", id: chosen.id });
          onClose();
        }}
      />
    );
  }

  return (
    <Dialog onClose={onClose}>
      <div className="flex flex-col gap-4">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <FileUp className="size-5" /> Open {fileName}
        </h2>
        {ready.length === 0 ? (
          <p className={subtle}>
            Windows programs run inside a bottle. Create one first, then open the file again.
          </p>
        ) : (
          <>
            <p className={subtle}>
              Choose the bottle to run it in. An installer puts its program in that bottle; nothing is
              installed on your Mac itself.
            </p>
            <Label title="Bottle">
              <Select
                value={chosen?.id ?? ""}
                onChange={setBottleId}
                options={ready.map((b) => ({ value: b.id, label: b.name }))}
              />
            </Label>
          </>
        )}
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          {ready.length === 0 ? (
            <Button
              variant="primary"
              onClick={() => {
                onNavigate({ page: "new-bottle" });
                onClose();
              }}
            >
              <Plus className="size-4" /> New bottle
            </Button>
          ) : (
            <Button variant="primary" onClick={() => setConfirmed(true)}>
              Continue
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
