import { useEffect, useRef, type ReactNode } from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), audio[controls], [tabindex]:not([tabindex="-1"])';

function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    // `display: none` (a hidden file input, an inactive tab) can't take focus.
    (el) => el.getClientRects().length > 0,
  );
}

/**
 * The backdrop and panel every dialog in the app shares, with the behaviour
 * a modal owes keyboard and screen-reader users: it announces itself as a
 * dialog, takes focus when it opens (onto the element marked
 * `data-autofocus`, else the first control), keeps Tab cycling inside it,
 * closes on Escape, and hands focus back to whatever opened it.
 *
 * `onClose` left undefined makes Escape and the backdrop do nothing — how a
 * dialog stays up while the request it started is still running.
 */
export default function Modal({
  labelledBy,
  onClose,
  closeOnBackdrop = false,
  className,
  children,
}: {
  /** Id of the element holding the dialog's title. */
  labelledBy: string;
  onClose?: () => void;
  /**
   * Off by default: a stray click beside the panel shouldn't throw away a
   * recording or a half-filled form. Worth turning on for dialogs that hold
   * nothing but a question.
   */
  closeOnBackdrop?: boolean;
  className: string;
  children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    const root = panel.current;
    if (root) {
      const target =
        root.querySelector<HTMLElement>("[data-autofocus]") ??
        focusables(root)[0] ??
        root;
      target.focus();
    }
    return () => opener?.focus?.();
  }, []);

  useEffect(() => {
    if (!onClose) return;
    const close = onClose;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") close();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function trapTab(e: React.KeyboardEvent) {
    if (e.key !== "Tab" || !panel.current) return;
    const items = focusables(panel.current);
    if (items.length === 0) {
      e.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const current = document.activeElement as HTMLElement | null;
    const inside = current !== null && items.includes(current);
    if (e.shiftKey && (!inside || current === first)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (!inside || current === last)) {
      e.preventDefault();
      first.focus();
    }
  }

  return (
    <div
      onMouseDown={(e) => {
        if (closeOnBackdrop && onClose && e.target === e.currentTarget) {
          onClose();
        }
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onKeyDown={trapTab}
        className={`outline-none ${className}`}
      >
        {children}
      </div>
    </div>
  );
}
