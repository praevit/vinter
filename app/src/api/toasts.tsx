import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { events } from "./bindings";

/** A button on a toast. Clicking it also dismisses the toast. */
export type ToastAction = { label: string; onClick: () => void };
type Toast = { id: number; message: string; tone: "error" | "info"; action?: ToastAction };
type Toasts = { show: (message: string, tone?: Toast["tone"], action?: ToastAction) => void };

const ToastContext = createContext<Toasts | null>(null);
let nextId = 1;

/**
 * Small notices in the corner: background errors (e.g. a launcher that
 * failed), failed jobs, and programs that exited with an error.
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const dismiss = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);
  const show = useCallback(
    (message: string, tone: Toast["tone"] = "error", action?: ToastAction) => {
      const id = nextId++;
      setToasts((t) => [...t.slice(-3), { id, message, tone, action }]);
      // An error with something to do about it stays until dismissed.
      if (!(action && tone === "error")) setTimeout(() => dismiss(id), 8000);
    },
    [dismiss],
  );

  useEffect(() => {
    const notice = events.appNotice.listen(({ payload }) => show(payload.message));
    const done = events.jobDone.listen(({ payload }) => {
      if (!payload.ok && payload.error) show(payload.error);
    });
    return () => {
      notice.then((u) => u());
      done.then((u) => u());
    };
  }, [show]);

  return (
    <ToastContext value={{ show }}>
      {children}
      <div
        className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2"
        role="status"
        aria-live="polite"
      >
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`pointer-events-auto flex items-start gap-2 rounded-xl border p-3 text-sm shadow-lg ${
              t.tone === "error"
                ? "border-wine-500/40 bg-white dark:bg-slate-deep-800"
                : "border-frost-200 bg-white dark:border-slate-deep-700 dark:bg-slate-deep-800"
            }`}
          >
            <div className="flex min-w-0 flex-1 flex-col items-start gap-2">
              <p className="line-clamp-4 select-text">{t.message}</p>
              {t.action && (
                <button
                  type="button"
                  onClick={() => {
                    dismiss(t.id);
                    t.action?.onClick();
                  }}
                  className="rounded-md border border-frost-300 px-2.5 py-1 text-xs font-medium hover:bg-frost-100 dark:border-slate-deep-700 dark:hover:bg-slate-deep-700"
                >
                  {t.action.label}
                </button>
              )}
            </div>
            <button type="button" aria-label="Dismiss" onClick={() => dismiss(t.id)} className="text-frost-400 hover:text-current">
              <X className="size-4" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext>
  );
}

export function useToasts(): Toasts {
  const t = useContext(ToastContext);
  if (!t) throw new Error("useToasts must be used inside <ToastProvider>");
  return t;
}
