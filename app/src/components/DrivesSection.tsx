import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, FolderPlus, HardDrive, Trash2 } from "lucide-react";
import { commands, type DriveMapping } from "../api/bindings";
import type { BottleDetails } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { Select } from "./Select";
import { Button, ErrorText, subtle } from "./ui";

/** Letters a folder can use: D to Y (C: is the bottle's own, Z: the whole computer). */
const LETTERS = "DEFGHIJKLMNOPQRSTUVWXY".split("");

/** Folders shown to the bottle's programs as drive letters, and hiding Z:. For a bottle's Settings tab. */
export function DrivesSection({ bottle }: { bottle: BottleDetails }) {
  const queryClient = useQueryClient();
  const save = useMutation({
    mutationFn: ({ drives, hide }: { drives: DriveMapping[]; hide: boolean }) =>
      commands.bottleSetDrives(bottle.id, drives, hide),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bottles"] }),
  });
  const drives = bottle.drives;
  const hide = bottle.hide_root_drive;
  const used = new Set(drives.map((d) => d.letter));

  const add = async () => {
    const path = await commands.driveChooseFolder().catch(() => null);
    const letter = LETTERS.find((l) => !used.has(l));
    if (!path || !letter) return;
    save.mutate({ drives: [...drives, { letter, path }], hide });
  };

  const setLetter = (index: number, letter: string) =>
    save.mutate({ drives: drives.map((d, i) => (i === index ? { ...d, letter } : d)), hide });

  return (
    <section className="flex flex-col gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">Drives</h2>
      <p className={subtle}>
        C: is the bottle's own drive. Add a folder as a drive to give Windows programs a short way to reach it.
      </p>
      <ul className="flex flex-col gap-1.5">
        <li className="flex items-center gap-2 text-sm">
          <HardDrive className="size-4 shrink-0 text-frost-400" />
          <span className="w-12 font-medium">C:</span>
          <span className={subtle}>This bottle</span>
        </li>
        {drives.map((d, i) => (
          <li key={d.letter} className="flex items-center gap-2 text-sm">
            <HardDrive className="size-4 shrink-0 text-frost-400" />
            <div className="w-20 shrink-0">
              <Select
                value={d.letter}
                disabled={save.isPending}
                options={LETTERS.filter((l) => l === d.letter || !used.has(l)).map((l) => ({
                  value: l,
                  label: `${l}:`,
                }))}
                onChange={(letter) => setLetter(i, letter)}
              />
            </div>
            <span className="min-w-0 flex-1 truncate select-text" title={d.path}>
              {d.path}
            </span>
            <button
              type="button"
              title="Remove this drive (the folder stays)"
              aria-label={`Remove ${d.letter}:`}
              disabled={save.isPending}
              onClick={() => save.mutate({ drives: drives.filter((_, j) => j !== i), hide })}
              className="rounded p-1 text-frost-400 hover:bg-frost-100 hover:text-wine-500 dark:hover:bg-slate-deep-800"
            >
              <Trash2 className="size-3.5" />
            </button>
          </li>
        ))}
        {!hide && (
          <li className="flex items-center gap-2 text-sm">
            <HardDrive className="size-4 shrink-0 text-frost-400" />
            <span className="w-12 font-medium">Z:</span>
            <span className={subtle}>Everything on this computer</span>
          </li>
        )}
      </ul>
      <div>
        <Button disabled={save.isPending || used.size >= LETTERS.length} onClick={add}>
          <FolderPlus className="size-4" /> Add folder as drive…
        </Button>
      </div>
      <label className="mt-1 flex items-start gap-2.5 text-sm">
        <input
          type="checkbox"
          className="mt-0.5 accent-wine-500"
          checked={hide}
          disabled={save.isPending}
          onChange={(e) => save.mutate({ drives, hide: e.target.checked })}
        />
        <span className="flex flex-col">
          <span className="font-medium">Only show the folders I choose</span>
          <span className={subtle}>Removes Z:, so programs can't see the rest of this computer.</span>
        </span>
      </label>
      {hide && (
        <p className={`${subtle} flex items-start gap-1.5`}>
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
          Programs can now only open files in C: and the folders above. Some programs need Z: to open or save
          files elsewhere on this computer, or to open files you drop on them.
        </p>
      )}
      <p className={subtle}>Running programs may not see changes until they restart.</p>
      {save.error && <ErrorText>{errorMessage(save.error)}</ErrorText>}
    </section>
  );
}
