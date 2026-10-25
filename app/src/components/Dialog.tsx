import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Open dialogs, oldest first: only the last one takes keys. */
const stack: string[] = [];

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
const FIELDS = 'input:not([disabled]):not([type="hidden"]):not([type="checkbox"]):not([type="radio"]), textarea:not([disabled])';

/**
 * A centered modal. Escape and the backdrop call `onClose`, if given. Focus
 * moves into the dialog (to its first text field, or the dialog itself), stays
 * there while it is open, and goes back where it was when it closes. With
 * `title` it gets a heading; with `actions`, a row of buttons at the bottom.
 * Without `title`, the first heading inside names the dialog.
 */
export function Dialog({
  children,
  onClose,
  title,
  actions,
}: {
  children: ReactNode;
  onClose?: () => void;
  title?: ReactNode;
  actions?: ReactNode;
}) {
  const id = useId();
  const titleId = `${id}-title`;
  const panel = useRef<HTMLDivElement>(null);
  // Read during render, before any `autoFocus` inside moves focus.
  const returnTo = useRef<Element | null>(document.activeElement);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    stack.push(id);
    const el = panel.current;
    if (el) {
      // Name the dialog after its heading.
      if (!el.hasAttribute("aria-labelledby")) {
        const heading = el.querySelector("h1, h2, h3");
        if (heading) {
          if (!heading.id) heading.id = titleId;
          el.setAttribute("aria-labelledby", heading.id);
        }
      }
      if (!el.contains(document.activeElement)) {
        (el.querySelector<HTMLElement>(FIELDS) ?? el).focus();
      }
    }

    const onKey = (e: KeyboardEvent) => {
      if (stack[stack.length - 1] !== id || !panel.current) return;
      if (e.key === "Escape") {
        if (closeRef.current) {
          e.preventDefault();
          closeRef.current();
        }
      } else if (e.key === "Tab") {
        const items = [...panel.current.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
          (x) => x.offsetParent !== null || x === document.activeElement,
        );
        if (items.length === 0) {
          e.preventDefault();
          panel.current.focus();
          return;
        }
        const first = items[0];
        const last = items[items.length - 1];
        const inside = panel.current.contains(document.activeElement);
        if (e.shiftKey && (!inside || document.activeElement === first || document.activeElement === panel.current)) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && (!inside || document.activeElement === last)) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    const back = returnTo.current;
    return () => {
      window.removeEventListener("keydown", onKey);
      const at = stack.lastIndexOf(id);
      if (at >= 0) stack.splice(at, 1);
      if (back instanceof HTMLElement && back.isConnected) back.focus();
    };
  }, [id, titleId]);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-deep-950/50 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? titleId : undefined}
        tabIndex={-1}
        className="w-full max-w-md rounded-2xl border border-frost-200 bg-white p-6 shadow-xl outline-none dark:border-slate-deep-700 dark:bg-slate-deep-900"
        onClick={(e) => e.stopPropagation()}
      >
        {title || actions ? (
          <div className="flex flex-col gap-4">
            {title && (
              <h2 id={titleId} className="text-lg font-semibold">
                {title}
              </h2>
            )}
            {children}
            {actions && <div className="flex flex-wrap justify-end gap-2">{actions}</div>}
          </div>
        ) : (
          children
        )}
      </div>
    </div>,
    document.body,
  );
}
