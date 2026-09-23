/**
 * Renders a stored timestamp — RFC 3339 from our own database, or the
 * `YYYY-MM-DD HH:MM:SS` DashScope reports voices with — as a short local
 * date and time. An unparseable value falls back to the raw string, which is
 * at least still readable.
 */
export function formatDateTime(raw: string, locale: string): string {
  const at = new Date(raw);
  if (Number.isNaN(at.getTime())) return raw;
  return at.toLocaleString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Each unit, and how many of it make up the next one. */
const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["minute", 60],
  ["hour", 24],
  ["day", 7],
  ["week", 30 / 7],
  ["month", 12],
  ["year", Infinity],
];

/**
 * How long ago a stored timestamp was, the way a person says it: "3 hours
 * ago", "昨天". Falls back to the raw string when it doesn't parse.
 */
export function formatRelative(raw: string, locale: string): string {
  const at = new Date(raw);
  if (Number.isNaN(at.getTime())) return raw;
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  let amount = (at.getTime() - Date.now()) / 60_000;
  for (const [unit, perNext] of RELATIVE_UNITS) {
    if (Math.abs(amount) < perNext) {
      return rtf.format(Math.round(amount), unit);
    }
    amount /= perNext;
  }
  return raw;
}

/** Physical key codes whose name says nothing about the key's printed label. */
const KEY_LABEL: Record<string, string> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

const IS_WINDOWS = navigator.userAgent.includes("Windows");

/**
 * An accelerator as stored (`Ctrl+Shift+KeyM`, built from `KeyboardEvent.code`
 * so the backend can register it) rewritten the way a keyboard labels it
 * (`Ctrl + Shift + M`). Display only — the stored form is what gets saved.
 */
export function formatHotkey(accelerator: string): string {
  return accelerator
    .split("+")
    .map((part) => {
      if (/^Key[A-Z]$/.test(part)) return part.slice(3);
      if (/^Digit\d$/.test(part)) return part.slice(5);
      if (part.startsWith("Numpad")) return `Num ${part.slice(6)}`;
      if (part === "Super") return IS_WINDOWS ? "Win" : "Super";
      return KEY_LABEL[part] ?? part;
    })
    .join(" + ");
}
