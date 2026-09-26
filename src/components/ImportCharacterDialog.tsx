import { useEffect, useId, useState } from "react";
import { Import } from "lucide-react";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { describeVoice, LANGUAGE_LABEL } from "../lib/labels";
import type { Character, SharedCharacter } from "../lib/types";
import { btn } from "../lib/ui";
import Avatar from "./Avatar";
import Modal from "./Modal";

/**
 * Shows what a shared character file holds and imports it on confirmation.
 *
 * The file has already been read and checked (`open_character_file`); what
 * remains is the part that costs something — making the voice under this
 * user's account, which for anything but a preset is a request or two to
 * DashScope and can take a while — so it waits for the user to see who they
 * are importing and say yes.
 */
export default function ImportCharacterDialog({
  shared,
  onImported,
  onClose,
}: {
  shared: SharedCharacter;
  onImported: (character: Character) => void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const [busy, setBusy] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const { voice } = shared;
  const makesVoice = voice.kind !== "preset";

  useEffect(() => {
    if (!busy) return;
    const started = Date.now();
    setSeconds(0);
    const timer = window.setInterval(
      () => setSeconds(Math.floor((Date.now() - started) / 1000)),
      250,
    );
    return () => window.clearInterval(timer);
  }, [busy]);

  async function handleImport() {
    setBusy(true);
    setError(null);
    try {
      onImported(await ipc.importCharacter(shared));
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <Modal
      labelledBy={titleId}
      // Stays up while the voice is being made: closing would hide a request
      // that is still going to create a character.
      onClose={busy ? undefined : onClose}
      className="flex max-h-full w-full max-w-lg flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100"
    >
      <div className="border-b border-neutral-800 px-5 py-3">
        <h2 id={titleId} className="text-sm font-medium">
          {t("import.title")}
        </h2>
      </div>

      <div className="flex-1 space-y-5 overflow-y-auto p-5">
        <header className="flex items-center gap-4">
          {shared.avatar ? (
            <img
              src={shared.avatar}
              alt=""
              draggable={false}
              className="size-16 shrink-0 rounded-full bg-neutral-800 object-cover"
            />
          ) : (
            <Avatar id="" name={shared.name} avatarPath={null} size="lg" />
          )}
          <div className="min-w-0">
            <p className="text-lg font-semibold break-words">{shared.name}</p>
            <p className="text-sm text-neutral-400">
              {t(LANGUAGE_LABEL[shared.language])}
            </p>
          </div>
        </header>

        <Prose title={t("characterEdit.persona")}>
          {shared.persona || t("characters.noPersona")}
        </Prose>
        <Prose title={t("characterEdit.speechHabits")}>
          {shared.speech_habits || t("characters.noSpeechHabits")}
        </Prose>

        <section className="space-y-2 rounded-xl border border-neutral-800 bg-neutral-900/40 p-4">
          <h3 className="text-xs text-neutral-400">{t("characterEdit.voice")}</h3>
          {voice.kind === "preset" && (
            <p className="text-sm text-neutral-300">
              {t("import.voice.preset", {
                voice: describeVoice(t, "preset", voice.id),
              })}
            </p>
          )}
          {voice.kind === "description" && (
            <>
              <p className="text-sm text-neutral-300">
                {t("import.voice.description")}
              </p>
              <blockquote className="border-l-2 border-neutral-700 pl-3 text-sm whitespace-pre-wrap break-words text-neutral-400">
                {voice.prompt}
              </blockquote>
            </>
          )}
          {voice.kind === "audio" && (
            <>
              <p className="text-sm text-neutral-300">
                {t("import.voice.audio")}
              </p>
              {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
              <audio controls src={voice.data} className="w-full" />
            </>
          )}
          {makesVoice && (
            <p className="text-xs leading-relaxed text-neutral-500">
              {t("import.voice.account")}
            </p>
          )}
        </section>

        <p className="text-xs leading-relaxed text-neutral-500">
          {t("import.trust")}
        </p>

        {error && (
          <p role="alert" className="text-sm text-red-400">
            {error}
          </p>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 border-t border-neutral-800 px-5 py-3">
        {busy && makesVoice && (
          <span className="mr-auto text-xs text-neutral-500">
            {t("import.workingHint")}
          </span>
        )}
        <button
          onClick={onClose}
          disabled={busy}
          className={`${btn.outline} px-3 py-1.5 text-sm`}
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={handleImport}
          disabled={busy}
          data-autofocus
          className={`${btn.primary} gap-1.5 px-3 py-1.5 text-sm font-medium`}
        >
          <Import className={`size-3.5 ${busy ? "animate-pulse" : ""}`} />
          {!busy
            ? t("import.confirm")
            : makesVoice
              ? t("import.makingVoice", { seconds })
              : t("common.working")}
        </button>
      </div>
    </Modal>
  );
}

function Prose({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h3 className="mb-1.5 text-xs font-medium text-neutral-500">{title}</h3>
      <p className="max-h-40 overflow-y-auto text-sm leading-relaxed whitespace-pre-wrap break-words text-neutral-300">
        {children}
      </p>
    </section>
  );
}
