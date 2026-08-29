import { useEffect } from "react";

/**
 * Modal confirmation for actions that cannot be undone.
 *
 * Deliberately not `window.confirm()`: that blocks the webview's event loop
 * (so nothing repaints and incoming Tauri events queue up behind it), can't be
 * styled to match the rest of the app, and renders inconsistently inside
 * WebView2.
 */
export default function ConfirmDialog({
  title,
  body,
  confirmLabel = "删除",
  busy = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  body?: string;
  confirmLabel?: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      // Escape only backs out while idle — cancelling mid-request would hide
      // the dialog while the deletion it started is still running.
      if (e.key === "Escape" && !busy) onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="w-full max-w-sm rounded-2xl border border-neutral-800 bg-neutral-950 p-5 text-neutral-100">
        <h2 className="text-sm font-medium">{title}</h2>
        {body && (
          <p className="mt-2 text-sm leading-relaxed text-neutral-400">{body}</p>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onCancel}
            disabled={busy}
            className="rounded border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 disabled:opacity-40"
          >
            取消
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            className="rounded bg-red-500 px-3 py-1.5 text-sm font-medium text-neutral-950 disabled:opacity-50"
          >
            {busy ? "处理中…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
