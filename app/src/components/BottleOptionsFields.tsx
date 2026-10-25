import type { BottleOptions_Serialize as BottleOptions, WineDebug } from "../api/bindings";
import { Select } from "./Select";
import { subtle } from "./ui";

type Switch = "retina_mode" | "command_as_ctrl" | "option_as_alt" | "show_fps";

const WINE_DEBUG: { value: WineDebug; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "errors", label: "Errors (recommended)" },
  { value: "verbose", label: "Verbose (for troubleshooting)" },
];

const OPTIONS: { key: Switch; label: string; help: string; macOnly: boolean }[] = [
  {
    key: "retina_mode",
    label: "Retina mode",
    help: "Sharp text and graphics on Retina screens. Some older programs may look small.",
    macOnly: true,
  },
  {
    key: "command_as_ctrl",
    label: "⌘ works as Ctrl",
    help: "⌘C, ⌘V and other shortcuts work as on a Mac.",
    macOnly: true,
  },
  {
    key: "option_as_alt",
    label: "⌥ works as Alt",
    help: "For Alt shortcuts and menus. Turn it off if your keyboard uses ⌥ to type characters such as @.",
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
      <div className="flex flex-col gap-1.5 text-sm">
        <span className="font-medium">Wine logging</span>
        <Select
          value={value.wine_debug}
          disabled={disabled}
          options={WINE_DEBUG}
          onChange={(wine_debug) => onChange({ ...value, wine_debug })}
        />
        <span className={subtle}>
          What Wine writes to each program's log. Errors lets the log doctor find problems. Turn on Verbose when
          something fails, then open the log. Verbose logs get large.
        </span>
      </div>
    </div>
  );
}
