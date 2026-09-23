import { useId, type ReactNode } from "react";
import { useT } from "../lib/i18n";
import { btn } from "../lib/ui";
import Modal from "./Modal";

/**
 * Modal confirmation for an action worth stopping to think about: one that
 * cannot be undone, or one whose consequences leave the app.
 *
 * Deliberately not `window.confirm()`: that blocks the webview's event loop
 * (so nothing repaints and incoming Tauri events queue up behind it), can't be
 * styled to match the rest of the app, and renders inconsistently inside
 * WebView2.
 */
export default function ConfirmDialog({
  title,
  body,
  confirmLabel,
  tone = "danger",
  busy = false,
  children,
  onConfirm,
  onCancel,
}: {
  title: string;
  body?: string;
  /** Defaults to the translated "Delete", the action this is usually for. */
  confirmLabel?: string;
  /**
   * Red is for the deletions this dialog usually guards. A confirmation that
   * only asks the user to be deliberate — backing up, restoring — takes the
   * ordinary light button: dressing it as a destructive action reads as a
   * warning about something that isn't going to happen.
   */
  tone?: "danger" | "primary";
  busy?: boolean;
  /**
   * Controls belonging to the decision itself, shown under the body — the
   * backup dialog's "include my API key" is one. Anything the user would
   * still want after dismissing the dialog belongs on the page instead.
   */
  children?: ReactNode;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const titleId = useId();

  return (
    <Modal
      labelledBy={titleId}
      // Only backs out while idle — cancelling mid-request would hide the
      // dialog while the deletion it started is still running.
      onClose={busy ? undefined : onCancel}
      closeOnBackdrop
      className="w-full max-w-sm rounded-2xl border border-neutral-800 bg-neutral-950 p-5 text-neutral-100"
    >
      <h2 id={titleId} className="text-sm font-medium">
        {title}
      </h2>
      {body && (
        // `whitespace-pre-line` so a body can be more than one paragraph:
        // the restore warning splits "what this does" from "where the file
        // should come from", and run together they read as one hedge.
        <p className="mt-2 text-sm leading-relaxed whitespace-pre-line text-neutral-400">
          {body}
        </p>
      )}
      {children && <div className="mt-4">{children}</div>}
      <div className="mt-5 flex justify-end gap-2">
        {/* Focus starts on the safe choice: Enter right after a destructive
            dialog opens should back out, not delete. A dialog that only
            asks for deliberateness can start on its confirm instead. */}
        <button
          onClick={onCancel}
          disabled={busy}
          data-autofocus={tone === "danger" ? true : undefined}
          className={`${btn.outline} px-3 py-1.5 text-sm`}
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={onConfirm}
          disabled={busy}
          data-autofocus={tone === "danger" ? undefined : true}
          className={`${btn[tone]} px-3 py-1.5 text-sm font-medium`}
        >
          {busy ? t("common.working") : (confirmLabel ?? t("common.delete"))}
        </button>
      </div>
    </Modal>
  );
}
