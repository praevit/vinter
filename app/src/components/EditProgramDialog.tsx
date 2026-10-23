import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ImageUp } from "lucide-react";
import { commands, type ProgramView } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { CompatNoteSection } from "./CompatNoteSection";
import { Dialog } from "./Dialog";
import { ProgramIcon } from "./InstallDialog";
import { Button, ErrorText, Label, field, subtle } from "./ui";

/** Splits arguments on spaces, keeping "quoted parts" together. */
export function splitArgs(line: string): string[] {
  return [...line.matchAll(/"([^"]*)"|(\S+)/g)].map((m) => m[1] ?? m[2]);
}

/** The reverse of `splitArgs`: quotes arguments that contain spaces. */
export function joinArgs(args: string[]): string {
  return args.map((a) => (/\s/.test(a) || a === "" ? `"${a}"` : a)).join(" ");
}

export function EditProgramDialog({
  bottleId,
  program,
  onClose,
}: {
  bottleId: string;
  program: ProgramView;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(program.name);
  const [args, setArgs] = useState(joinArgs(program.args));
  const [icon, setIcon] = useState(program.icon_data_url);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  const refresh = () => queryClient.invalidateQueries({ queryKey: ["bottles"] });

  const chooseIcon = async () => {
    setError(undefined);
    try {
      const bottle = await commands.programChooseIcon(bottleId, program.index);
      if (bottle) {
        await refresh();
        const list = await commands.programsList(bottleId);
        setIcon(list.find((p) => p.index === program.index)?.icon_data_url ?? null);
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const save = async () => {
    setSaving(true);
    setError(undefined);
    try {
      await commands.programUpdate(bottleId, program.index, name, splitArgs(args));
      await refresh();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog onClose={onClose}>
      <form
        className="flex flex-col gap-4 text-left"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2 className="text-lg font-semibold">Edit program</h2>
        <div className="flex items-center gap-3">
          <ProgramIcon src={icon} size="size-12" />
          <Button type="button" onClick={chooseIcon}>
            <ImageUp className="size-4" /> Change icon…
          </Button>
        </div>
        <Label title="Name">
          <input className={field} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Label>
        <Label title="Launch arguments">
          <input
            className={field}
            value={args}
            placeholder="none"
            onChange={(e) => setArgs(e.target.value)}
            spellCheck={false}
          />
        </Label>
        <p className={`${subtle} -mt-2 select-text break-all`}>{program.exe}</p>
        <CompatNoteSection bottleId={bottleId} program={program} />
        {error && <ErrorText>{error}</ErrorText>}
        <div className="flex justify-end gap-2">
          <Button type="button" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={saving || !name.trim()}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
