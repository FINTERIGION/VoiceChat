import type { ReactNode } from "react";

/**
 * A labelled form control. The control goes inside the `<label>`, which is
 * what ties the two together — clicking the caption focuses the field, and
 * a screen reader announces it — without an id to keep in sync.
 *
 * Only for a single control: a button inside a label would be activated by
 * clicks on the caption too.
 */
export default function Field({
  label,
  hint,
  className = "",
  children,
}: {
  label: ReactNode;
  hint?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <label className={`block ${className}`}>
      <span className="mb-1.5 block text-xs text-neutral-400">{label}</span>
      {children}
      {hint && (
        <span className="mt-1.5 block text-xs leading-relaxed text-neutral-500">
          {hint}
        </span>
      )}
    </label>
  );
}
