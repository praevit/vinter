import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { commands, type CompatNote, type ProgramView, type Rating } from "../api/bindings";
import { useCompatNotes } from "../api/compat";
import { errorMessage } from "../api/errors";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { Select } from "./Select";
import { Button, ErrorText, field, subtle } from "./ui";

export const RATINGS: { value: Rating; label: string; color: string }[] = [
  { value: "works", label: "Works", color: "bg-emerald-500" },
  { value: "partly", label: "Partly works", color: "bg-amber-400" },
  { value: "broken", label: "Broken", color: "bg-rose-500" },
];

/** The note for one program, if any. */
export function useProgramNote(bottleId: string, program: ProgramView): CompatNote | undefined {
  const { notes } = useCompatNotes();
  return notes.find((n) => n.bottle_id === bottleId && n.exe === program.exe);
}

/** A program's own compatibility note (experimental `compat-notes`). */
export function CompatNoteSection({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const { enabled } = useCompatNotes();
  const saved = useProgramNote(bottleId, program);
  const queryClient = useQueryClient();
  const [rating, setRating] = useState<Rating>(saved?.rating ?? "works");
  const [note, setNote] = useState(saved?.note ?? "");
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  if (!enabled) return null;

  const run = async (action: () => Promise<unknown>, done: string) => {
    setError(undefined);
    try {
      await action();
      setMessage(done);
      await queryClient.invalidateQueries({ queryKey: ["compat-notes"] });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <section className="flex flex-col gap-2 rounded-xl border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        Compatibility note <ExperimentalBadge />
      </h3>
      <Select value={rating} options={RATINGS} onChange={setRating} />
      <textarea
        className={`${field} min-h-16`}
        value={note}
        maxLength={2000}
        placeholder="What works, what doesn't, what you changed"
        onChange={(e) => setNote(e.target.value)}
      />
      <p className={subtle}>
        Kept on this Mac only, with this bottle's engine and graphics backend. Nothing is sent anywhere.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          onClick={() => run(() => commands.compatNoteSet(bottleId, program.index, rating, note), "Note saved.")}
        >
          Save note
        </Button>
        {saved && (
          <Button
            type="button"
            variant="danger"
            onClick={() => run(() => commands.compatNoteRemove(bottleId, program.index), "Note removed.")}
          >
            Remove
          </Button>
        )}
        <Button
          type="button"
          onClick={() =>
            run(async () => {
              const path = await commands.compatNotesExport();
              if (!path) throw new Error("Not exported.");
              setMessage(`Saved to ${path}.`);
            }, "Exported.")
          }
        >
          Export all as JSON…
        </Button>
        {message && <span className={subtle}>{message}</span>}
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}

/** A small dot on a program's tile with its rating. */
export function CompatNoteDot({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const note = useProgramNote(bottleId, program);
  if (!note) return null;
  const r = RATINGS.find((x) => x.value === note.rating);
  return (
    <span
      title={`${r?.label}${note.note ? `: ${note.note}` : ""} (your note, experimental)`}
      className={`absolute top-2 right-8 size-2 rounded-full ${r?.color}`}
    />
  );
}
