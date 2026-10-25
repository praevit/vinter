import type { ButtonHTMLAttributes, ReactNode } from "react";

/** Shared look for buttons and form fields. */
export const subtle = "text-xs text-secondary";
export const field =
  "w-full rounded-md border border-frost-300 bg-white px-2.5 py-1.5 text-sm focus:border-wine-500 dark:border-slate-deep-700 dark:bg-slate-deep-950";

type Variant = "primary" | "plain" | "danger";
const variants: Record<Variant, string> = {
  primary: "bg-wine-500 text-white hover:bg-wine-600",
  plain:
    "border border-frost-300 hover:bg-frost-100 dark:border-slate-deep-700 dark:hover:bg-slate-deep-800",
  danger: "text-wine-500 hover:bg-frost-100 dark:text-rose-300 dark:hover:bg-slate-deep-800",
};

export function Button({
  variant = "plain",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      type="button"
      className={`flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm disabled:opacity-50 ${variants[variant]} ${className}`}
      {...props}
    />
  );
}

export function Label({ title, children }: { title: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-sm font-medium">{title}</span>
      {children}
    </label>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return <p className="text-sm text-wine-500 select-text dark:text-rose-400">{children}</p>;
}
