/**
 * Shared button styling.
 *
 * Every button in the app reacts the same way: it brightens and casts a
 * shadow under the cursor, presses in on click, and shows a ring when focused
 * from the keyboard. The motion is gated on `not-disabled:` rather than switched
 * off with `pointer-events-none`, so a disabled button stays inert but still
 * shows the `title` tooltip explaining why it is disabled.
 *
 * A variant carries the colours, the corner radius and the motion — nothing
 * else. Padding and font size stay at the call site, where the surrounding
 * layout decides them, and adding either here would collide with the
 * call site's own utility (which of two conflicting classes wins is decided
 * by stylesheet order, not by the order they appear in the string):
 *
 *     className={`${btn.primary} w-full py-2.5 text-sm`}
 */

/**
 * Motion and interaction states alone — no display, shape or colour. For
 * clickable things that are not buttons and bring their own layout: a list
 * row, a whole card.
 */
export const interactive =
  "select-none outline-none transition duration-200 focus-visible:ring-2 focus-visible:ring-neutral-500 not-disabled:cursor-pointer disabled:cursor-not-allowed disabled:opacity-40";

/**
 * The same, laid out as a button and pressing in when clicked: the base for
 * the variants below, and for the handful of buttons that carry their own
 * look (toggles that swap colour with their state, pill-shaped switches).
 */
export const btnBase = `inline-flex items-center justify-center ${interactive} not-disabled:active:scale-[0.97]`;

/**
 * The hover glow. Left off the small borderless actions inside a card, which
 * have no edge to cast it.
 */
export const hoverGlow = "not-disabled:hover:shadow-lg";

/**
 * The glow plus a lift, reserved for whole rows that are themselves the
 * button — a character card, a voice preset. One big card coming forward
 * reads as the row picking itself out; a screenful of buttons doing it one
 * after another just reads as restless, so ordinary buttons stay put.
 */
export const hoverLift = `${hoverGlow} not-disabled:hover:-translate-y-0.5 not-disabled:active:translate-y-0`;

const BASE = `${btnBase} rounded-lg`;

export const btn = {
  /** Filled and light: the one action a screen is really for. */
  primary: `${BASE} ${hoverGlow} bg-neutral-100 text-neutral-900 not-disabled:hover:bg-white not-disabled:hover:shadow-black/40`,

  /** Filled and dark: a secondary action that still wants some weight. */
  solid: `${BASE} ${hoverGlow} bg-neutral-800 text-neutral-100 not-disabled:hover:bg-neutral-700 not-disabled:hover:shadow-black/40`,

  /** Outlined: the everyday secondary action. */
  outline: `${BASE} ${hoverGlow} border border-neutral-700 text-neutral-200 not-disabled:hover:border-neutral-500 not-disabled:hover:bg-neutral-800/60 not-disabled:hover:shadow-black/40`,

  /** Borderless, tinted on hover: row actions living inside a card. */
  ghost: `${BASE} text-neutral-400 not-disabled:hover:bg-neutral-800 not-disabled:hover:text-neutral-100`,

  /** Borderless and green: confirming an inline edit. */
  accentGhost: `${BASE} text-emerald-400 not-disabled:hover:bg-emerald-500/10 not-disabled:hover:text-emerald-300`,

  /** Filled and red: the confirm button of a confirmation dialog. */
  danger: `${BASE} ${hoverGlow} bg-red-500 text-neutral-950 not-disabled:hover:bg-red-400 not-disabled:hover:shadow-red-500/25`,

  /** Outlined and red: a destructive action offered alongside ordinary ones. */
  dangerOutline: `${BASE} ${hoverGlow} border border-neutral-800 text-red-400 not-disabled:hover:border-red-500/40 not-disabled:hover:bg-red-500/10 not-disabled:hover:text-red-300 not-disabled:hover:shadow-red-500/10`,

  /** Borderless, turning red on hover: the delete in a card's row of actions. */
  dangerGhost: `${BASE} text-neutral-400 not-disabled:hover:bg-red-500/10 not-disabled:hover:text-red-400`,

  /** Plain text: back links, refresh, a dialog's close cross. */
  quiet: `${BASE} text-neutral-500 not-disabled:hover:text-neutral-200`,
};

/** A whole list row that is itself the button — a voice preset, say. */
export const cardBtn = `${interactive} block w-full rounded-xl border border-neutral-800 bg-neutral-950 text-left ${hoverLift} not-disabled:hover:border-neutral-700 not-disabled:hover:bg-neutral-900 not-disabled:hover:shadow-black/40 not-disabled:active:scale-[0.995]`;

/** Tab strips sit on a border, so they slide colour rather than lift. */
export function tabBtn(active: boolean) {
  return `${btnBase} rounded-t-lg px-3 py-1.5 text-sm ${
    active
      ? "bg-neutral-900 text-neutral-100"
      : "text-neutral-500 not-disabled:hover:bg-neutral-900/60 not-disabled:hover:text-neutral-300"
  }`;
}
