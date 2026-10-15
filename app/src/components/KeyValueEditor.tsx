import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";
import { Button, ErrorText, field, subtle } from "./ui";

type Row = { key: string; value: string };

/** Edits a string map as rows; Save sends the whole map. */
export function KeyValueEditor({
  title,
  help,
  keyPlaceholder,
  valuePlaceholder,
  values,
  saving,
  error,
  onSave,
}: {
  title: string;
  help: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  values: Record<string, string>;
  saving: boolean;
  error?: string;
  onSave: (values: Record<string, string>) => void;
}) {
  const toRows = (v: Record<string, string>) => Object.entries(v).map(([key, value]) => ({ key, value }));
  const [rows, setRows] = useState<Row[]>(toRows(values));
  // Reset when the saved values change (e.g. after saving).
  const saved = JSON.stringify(values);
  useEffect(() => setRows(toRows(JSON.parse(saved))), [saved]);

  const dirty = JSON.stringify(Object.fromEntries(rows.filter((r) => r.key.trim()).map((r) => [r.key.trim(), r.value]))) !== saved;
  const update = (i: number, row: Partial<Row>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...row } : r)));

  return (
    <div className="flex flex-col gap-2">
      <div>
        <h3 className="text-sm font-medium">{title}</h3>
        <p className={subtle}>{help}</p>
      </div>
      {rows.map((row, i) => (
        <div key={i} className="flex gap-2">
          <input
            className={`${field} font-mono`}
            value={row.key}
            placeholder={keyPlaceholder}
            aria-label={`${title} name ${i + 1}`}
            onChange={(e) => update(i, { key: e.target.value })}
          />
          <input
            className={`${field} font-mono`}
            value={row.value}
            placeholder={valuePlaceholder}
            aria-label={`${title} value ${i + 1}`}
            onChange={(e) => update(i, { value: e.target.value })}
          />
          <Button aria-label="Remove" onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
            <X className="size-4" />
          </Button>
        </div>
      ))}
      <div className="flex gap-2">
        <Button onClick={() => setRows((rs) => [...rs, { key: "", value: "" }])}>
          <Plus className="size-4" /> Add
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || saving}
          onClick={() =>
            onSave(Object.fromEntries(rows.filter((r) => r.key.trim()).map((r) => [r.key.trim(), r.value])))
          }
        >
          Save
        </Button>
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
