import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type FexOption, type ProgramView } from "../api/bindings";
import { useBottles } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { useExperimental } from "../api/experimental";
import { ExperimentalBadge } from "./ExperimentalBadge";
import { ErrorText, field, subtle } from "./ui";

/** A program's own FEX settings, from FEX's schema (experimental `fex-config-editor`). */
export function FexSection({ bottleId, program }: { bottleId: string; program: ProgramView }) {
  const enabled = useExperimental("fex-config-editor");
  const { data: bottles } = useBottles();
  const fex = bottles?.find((b) => b.id === bottleId)?.engine === "arm64-fex";
  const queryClient = useQueryClient();
  const options = useQuery({
    queryKey: ["fex-options", bottleId],
    queryFn: () => commands.fexOptions(bottleId),
    enabled: enabled && fex,
  });
  const key = ["fex-settings", bottleId, program.index];
  const { data: settings } = useQuery({
    queryKey: key,
    queryFn: () => commands.fexProgramSettings(bottleId, program.index),
    enabled: enabled && fex,
  });
  const [error, setError] = useState<string>();
  if (!enabled || !fex) return null;

  const save = async (name: string, value: string | null) => {
    setError(undefined);
    try {
      queryClient.setQueryData(key, await commands.fexSettingSet(bottleId, program.index, name, value));
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  const groups = new Map<string, FexOption[]>();
  for (const o of options.data ?? []) groups.set(o.group, [...(groups.get(o.group) ?? []), o]);

  return (
    <section className="flex flex-col gap-2 rounded-xl border border-frost-200 p-3 dark:border-slate-deep-700">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        FEX settings <ExperimentalBadge />
      </h3>
      <p className={subtle}>
        High risk: these change how Intel code is translated for this app only. Wrong values can make it crash or run
        slowly. Leave an option on Default to use FEX's own.
      </p>
      {options.error && <ErrorText>{errorMessage(options.error)}</ErrorText>}
      <div className="max-h-72 overflow-y-auto">
        {[...groups].map(([group, list]) => (
          <details key={group} className="border-t border-frost-200 py-1 dark:border-slate-deep-700">
            <summary className="cursor-pointer text-sm font-medium">{group}</summary>
            {list.map((o) => (
              <FexRow key={o.name} option={o} value={settings?.[o.name]} onSave={save} />
            ))}
          </details>
        ))}
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}

function FexRow({
  option: o,
  value,
  onSave,
}: {
  option: FexOption;
  value: string | undefined;
  onSave: (name: string, value: string | null) => void;
}) {
  const [draft, setDraft] = useState(value ?? "");
  return (
    <div className="flex flex-col gap-1 py-2">
      <span className="text-sm">
        {o.name} <span className={subtle}>({o.schema_type}, default {o.default || "empty"})</span>
      </span>
      {o.description && <span className={subtle}>{o.description}</span>}
      {o.kind === "bool" ? (
        <select
          className={field}
          value={value ?? ""}
          onChange={(e) => onSave(o.name, e.target.value || null)}
        >
          <option value="">Default</option>
          <option value="1">On</option>
          <option value="0">Off</option>
        </select>
      ) : o.kind === "read_only" ? (
        <span className={subtle}>Not changeable from Vinter.</span>
      ) : (
        <input
          className={field}
          value={draft}
          placeholder="Default"
          inputMode={o.kind === "integer" ? "numeric" : undefined}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => draft !== (value ?? "") && onSave(o.name, draft.trim() ? draft : null)}
        />
      )}
    </div>
  );
}
