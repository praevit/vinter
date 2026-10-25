import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Check, ChevronsUpDown } from "lucide-react";

export type SelectOption<T extends string> = { value: T; label: string };

/** A dropdown in Vinter's style (instead of the system's native one). */
export function Select<T extends string>({
  value,
  options,
  onChange,
  disabled,
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();
  const current = options.find((o) => o.value === value);

  // Close on a click elsewhere.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const show = () => {
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    setOpen(true);
  };
  const pick = (o: SelectOption<T>) => {
    setOpen(false);
    if (o.value !== value) onChange(o.value);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (disabled) return;
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        show();
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(options.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (options[active]) pick(options[active]);
    } else if (e.key === "Tab") {
      setOpen(false);
    }
  };

  return (
    <div ref={root} className="relative" onKeyDown={onKeyDown}>
      <button
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show())}
        className={`flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm disabled:opacity-50 ${
          open
            ? "border-wine-500"
            : "border-frost-300 focus-visible:border-wine-500 dark:border-slate-deep-700"
        } bg-white dark:bg-slate-deep-950`}
      >
        <span className="min-w-0 flex-1 truncate">{current?.label ?? value}</span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-tertiary" aria-hidden />
      </button>
      {open && (
        <ul
          id={listId}
          role="listbox"
          className="absolute top-full right-0 left-0 z-50 mt-1 max-h-64 overflow-y-auto rounded-lg border border-frost-200 bg-white p-1 shadow-xl dark:border-slate-deep-700 dark:bg-slate-deep-900"
        >
          {options.map((o, i) => (
            <li
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(o)}
              className={`flex cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-sm ${
                i === active ? "bg-frost-100 dark:bg-slate-deep-800" : ""
              }`}
            >
              <Check className={`size-3.5 shrink-0 text-wine-500 dark:text-rose-300 ${o.value === value ? "" : "invisible"}`} />
              <span className="truncate">{o.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
