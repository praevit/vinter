import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { BookmarkPlus, FolderOpen, Share2 } from "lucide-react";
import { commands, type SavedRecipe } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { Dialog } from "./Dialog";
import { Button, ErrorText, Label, field, subtle } from "./ui";

/** "Save as recipe…" for a bottle's Settings tab. */
export function SaveRecipeButton({ bottleId, bottleName }: { bottleId: string; bottleName: string }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="flex flex-col items-start gap-2 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <h2 className="text-sm font-medium">Recipe</h2>
      <p className={subtle}>
        Save how this bottle is set up as a recipe, to set up the same app again in one click, or to share it.
      </p>
      <Button onClick={() => setOpen(true)}>
        <BookmarkPlus className="size-4" /> Save as recipe…
      </Button>
      {open && <SaveRecipeDialog bottleId={bottleId} bottleName={bottleName} onClose={() => setOpen(false)} />}
    </section>
  );
}

/**
 * Saves a bottle's setup (Windows version, graphics, components, DLL
 * overrides, environment, programs) as a recipe, then offers to share it.
 */
export function SaveRecipeDialog({
  bottleId,
  bottleName,
  onClose,
}: {
  bottleId: string;
  bottleName: string;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(bottleName);
  const [description, setDescription] = useState("");
  const [hint, setHint] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<SavedRecipe>();
  const [shared, setShared] = useState(false);
  const [error, setError] = useState<string>();

  const run = async (action: () => Promise<void>) => {
    setError(undefined);
    try {
      await action();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const save = () =>
    run(async () => {
      setSaving(true);
      try {
        setSaved(await commands.bottleSaveRecipe(bottleId, name, description, hint));
        await queryClient.invalidateQueries({ queryKey: ["recipes"] });
      } finally {
        setSaving(false);
      }
    });

  return (
    <Dialog onClose={saving ? undefined : onClose}>
      {!saved ? (
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Save as recipe</h2>
          <p className="text-sm">
            The recipe sets up a new bottle like this one: its Windows version, graphics, installed components, DLL
            overrides, environment variables and programs. Whoever uses it picks the app's installer.
          </p>
          <Label title="App name">
            <input className={field} value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          </Label>
          <Label title="Description">
            <input
              className={field}
              value={description}
              maxLength={300}
              placeholder="What it is, and anything to know"
              onChange={(e) => setDescription(e.target.value)}
            />
          </Label>
          <Label title="Where to get the installer">
            <input
              className={field}
              value={hint}
              maxLength={300}
              placeholder="Download the 64-bit installer from example.com"
              onChange={(e) => setHint(e.target.value)}
            />
          </Label>
          {error && <ErrorText>{error}</ErrorText>}
          <div className="flex justify-end gap-2">
            <Button onClick={onClose} disabled={saving}>
              Cancel
            </Button>
            <Button variant="primary" disabled={saving || !name.trim()} onClick={save}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          <h2 className="text-lg font-semibold">Recipe saved</h2>
          <p className="text-sm">
            “{saved.name}” is in Install an App now. It is saved at{" "}
            <span className="break-all select-text">{saved.path}</span>.
          </p>
          <pre className="max-h-56 overflow-auto rounded-lg bg-frost-100 p-3 text-xs whitespace-pre-wrap select-text dark:bg-slate-deep-950">
            {saved.toml}
          </pre>
          <p className={subtle}>
            Share opens a new issue on Vinter's GitHub page in your browser, with this recipe and your compatibility
            notes for this bottle filled in. Nothing is sent unless you submit it there.
          </p>
          {shared && <p className={subtle}>Opened in your browser.</p>}
          {error && <ErrorText>{error}</ErrorText>}
          <div className="flex flex-wrap justify-end gap-2">
            <Button onClick={() => run(() => commands.recipesOpenFolder().then(() => undefined))}>
              <FolderOpen className="size-4" /> Show in folder
            </Button>
            <Button
              onClick={() =>
                run(async () => {
                  await commands.recipeShare(bottleId, saved.name, saved.toml);
                  setShared(true);
                })
              }
            >
              <Share2 className="size-4" /> Share…
            </Button>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
