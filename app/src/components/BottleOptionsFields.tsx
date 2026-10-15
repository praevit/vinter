import type { BottleOptions } from "../api/bindings";
import { subtle } from "./ui";

const OPTIONS: { key: keyof BottleOptions; label: string; help: string; macOnly: boolean }[] = [
  {
    key: "retina_mode",
    label: "Retina mode",
    help: "Sharp text and graphics on Retina screens. Some older programs may look small.",
    macOnly: true,
  },
  {
    key: "command_as_ctrl",
    label: "⌘ works as Ctrl",
    help: "⌘C, ⌘V and other shortcuts work as on a Mac; ⌥ works as Alt.",
    macOnly: true,
  },
  {
    key: "show_fps",
    label: "Show frame rate",
    help: "A frame counter in games (with DXVK, DXMT or D3DMetal).",
    macOnly: false,
  },
];

/** Checkboxes for a bottle's options (also used for the defaults in Settings). */
export function BottleOptionsFields({
  value,
  onChange,
  os,
  disabled,
}: {
  value: BottleOptions;
  onChange: (next: BottleOptions) => void;
  os: string | undefined;
  disabled?: boolean;
}) {
  return (
    <div className="flex flex-col gap-3">
      {OPTIONS.filter((o) => !o.macOnly || os === "macos").map((o) => (
        <label key={o.key} className="flex items-start gap-2.5 text-sm">
          <input
            type="checkbox"
            className="mt-0.5 accent-wine-500"
            checked={value[o.key]}
            disabled={disabled}
            onChange={(e) => onChange({ ...value, [o.key]: e.target.checked })}
          />
          <span className="flex flex-col">
            <span className="font-medium">{o.label}</span>
            <span className={subtle}>{o.help}</span>
          </span>
        </label>
      ))}
    </div>
  );
}
