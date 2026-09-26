import { useEffect, useRef, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  ArrowLeft,
  ExternalLink,
  Inbox,
  Loader2,
  Mail,
  MailOpen,
  Paperclip,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import clsx from "clsx";
import { EmptyState } from "../components/Card";
import { usePanelStore } from "../store";
import { usePanelSettings } from "../lib/panelWidth";
import { usePrefs } from "../lib/prefs";
import { motion } from "motion/react";
import { invoke } from "@tauri-apps/api/core";
import {
  invalidateMail,
  shortDate,
  useMailActions,
  useMailList,
  useMailSettings,
  type Letter,
  type MailAction,
  type Summary,
} from "../mail/api";

const URL_RE = /(https?:\/\/[^\s<>"')\]]+)/g;

/** Plain text with clickable http(s) links; everything else stays text. */
function Linkified({ text }: { text: string }) {
  const parts = text.split(URL_RE);
  return (
    <>
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <a
            key={i}
            href={part}
            onClick={(e) => {
              e.preventDefault();
              openUrl(part).catch(console.error);
            }}
            className="break-all text-accent hover:underline"
          >
            {part}
          </a>
        ) : (
          part
        ),
      )}
    </>
  );
}

function IconButton({
  title,
  onClick,
  children,
  busy,
}: {
  title: string;
  onClick: () => void;
  children: ReactNode;
  busy?: boolean;
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      disabled={busy}
      className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-[12px] text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-50"
    >
      {children}
    </button>
  );
}

function Reader({
  summary,
  multiAccount,
  split,
  onBack,
}: {
  summary: Summary;
  multiAccount: boolean;
  /** Shown next to the list rather than instead of it. */
  split: boolean;
  onBack: () => void;
}) {
  const { open, act } = useMailActions();
  const [letter, setLetter] = useState<Letter | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let current = true;
    open(summary).then(
      (l) => current && setLetter(l),
      (e) => current && setError(String(e)),
    );
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per letter
  }, [summary.account, summary.uid]);

  // Esc goes back to the list instead of closing the panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || usePanelStore.getState().query) return;
      e.stopImmediatePropagation();
      onBack();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onBack]);

  const run = async (action: MailAction) => {
    setBusy(true);
    try {
      await act(summary, action);
      onBack();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const shown = letter ?? summary;
  return (
    <div className="flex flex-col gap-3 pb-2">
      <div className="flex items-center gap-0.5">
        {split ? (
          <IconButton title="Закрыть письмо (Esc)" onClick={onBack}>
            <X className="size-4" /> Закрыть
          </IconButton>
        ) : (
          <IconButton title="Назад (Esc)" onClick={onBack}>
            <ArrowLeft className="size-4" /> Назад
          </IconButton>
        )}
        <div className="flex-1" />
        <IconButton title="В архив" onClick={() => run("archive")} busy={busy}>
          <Archive className="size-4" />
        </IconButton>
        <IconButton title="Удалить" onClick={() => run("delete")} busy={busy}>
          <Trash2 className="size-4" />
        </IconButton>
        <IconButton title="Отметить непрочитанным" onClick={() => run("unread")} busy={busy}>
          <Mail className="size-4" />
        </IconButton>
        {letter && (
          <IconButton title="Открыть в браузере" onClick={() => openUrl(letter.webUrl).catch(console.error)}>
            <ExternalLink className="size-4" />
          </IconButton>
        )}
      </div>

      <div className="rounded-2xl border border-stroke bg-surface p-3.5">
        <h2 className="text-[15px] leading-snug font-semibold select-text">{shown.subject}</h2>
        <div className="mt-2 flex items-baseline justify-between gap-3 text-[12px]">
          <div className="min-w-0">
            <div className="truncate text-fg select-text">
              {shown.fromName}
              {shown.fromEmail && shown.fromEmail !== shown.fromName && (
                <span className="text-fg-subtle"> &lt;{shown.fromEmail}&gt;</span>
              )}
            </div>
            {multiAccount && <div className="truncate text-fg-subtle">для {summary.account}</div>}
          </div>
          <span className="shrink-0 text-fg-subtle">
            {new Date(shown.date).toLocaleString("ru-RU", { dateStyle: "medium", timeStyle: "short" })}
          </span>
        </div>

        {letter && letter.attachments.length > 0 && (
          <div className="mt-2.5 flex flex-wrap gap-1.5">
            {letter.attachments.map((name, i) => (
              <span
                key={i}
                title="Вложения открываются в браузере"
                className="flex items-center gap-1 rounded-md border border-stroke px-1.5 py-0.5 text-[11.5px] text-fg-muted"
              >
                <Paperclip className="size-3" /> {name}
              </span>
            ))}
          </div>
        )}

        <div className="mt-3 border-t border-stroke pt-3 text-[13px] leading-relaxed">
          {error ? (
            <p className="text-warn">{error}</p>
          ) : letter ? (
            <div className="break-words whitespace-pre-wrap select-text">
              {letter.text ? <Linkified text={letter.text} /> : <span className="text-fg-subtle">(пустое письмо)</span>}
            </div>
          ) : (
            <p className="flex items-center gap-2 text-fg-subtle">
              <Loader2 className="size-4 animate-spin" /> Загружаю письмо…
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function Row({
  m,
  showAccount,
  active,
  selected,
  onOpen,
  onError,
}: {
  m: Summary;
  showAccount: boolean;
  /** The letter open next to the list. */
  active: boolean;
  /** The row the arrow keys are on. */
  selected: boolean;
  onOpen: () => void;
  onError: (message: string) => void;
}) {
  const { act } = useMailActions();
  // The row disappears at once (optimistic); a failure brings it back with a note.
  const run = (action: MailAction) => act(m, action).catch((e) => onError(String(e)));
  const quick = "grid size-7 place-items-center rounded-lg text-fg-subtle transition-colors hover:text-fg";
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <div
      ref={ref}
      className={clsx(
        "group relative rounded-xl transition-colors",
        active
          ? "bg-accent/12 ring-1 ring-accent/30 ring-inset"
          : selected
            ? "bg-ink/8"
            : "hover:bg-surface-hover",
      )}
    >
      <button onClick={onOpen} className="flex w-full items-start gap-2.5 px-2.5 py-2 text-left">
        <span className={clsx("mt-1.5 size-2 shrink-0 rounded-full", m.unread ? "bg-accent" : "bg-transparent")} />
        {/* On hover the text makes room for the quick actions. */}
        <div className="min-w-0 flex-1 group-hover:pr-14">
          <div className="flex items-baseline gap-2">
            <span className={clsx("min-w-0 flex-1 truncate text-[13.5px]", m.unread ? "font-semibold" : "text-fg-muted")}>
              {m.fromName || m.fromEmail || "(без отправителя)"}
            </span>
            <span className="shrink-0 text-[11.5px] text-fg-subtle tabular-nums group-hover:hidden">
              {shortDate(m.date)}
            </span>
          </div>
          <div className={clsx("truncate text-[12.5px]", m.unread ? "text-fg" : "text-fg-subtle")}>{m.subject}</div>
          {showAccount && <div className="truncate text-[11px] text-fg-subtle">{m.account}</div>}
        </div>
      </button>
      <div className="absolute inset-y-0 right-1.5 hidden items-center group-hover:flex">
        <button title="В архив" onClick={() => run("archive")} className={quick}>
          <Archive className="size-4" />
        </button>
        <button title="Удалить" onClick={() => run("delete")} className={quick}>
          <Trash2 className="size-4" />
        </button>
      </div>
    </div>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        "shrink-0 rounded-full border px-2.5 py-1 text-[12px] transition-colors",
        active ? "border-accent bg-accent/15 text-fg" : "border-stroke text-fg-muted hover:bg-ink/8 hover:text-fg",
      )}
    >
      {children}
    </button>
  );
}

/** Loads the next page when it scrolls into view (with some margin). */
function LoadMore({ onVisible, loading }: { onVisible: () => void; loading: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const latest = useRef(onVisible);
  latest.current = onVisible;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver((entries) => entries[0]?.isIntersecting && latest.current(), {
      rootMargin: "300px",
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={ref} className="flex h-10 items-center justify-center text-[12px] text-fg-subtle">
      {loading && (
        <span className="flex items-center gap-2">
          <Loader2 className="size-3.5 animate-spin" /> Загружаю ещё…
        </span>
      )}
    </div>
  );
}

/** The letter pane's width when the window grows for it. */
const READER_W = 560;
/** A panel at least this wide has room for the letter without growing. */
const WIDE_PANEL = 1050;
/** Below this much extra room the letter replaces the list instead. */
const READER_MIN = 360;

/** The list's narrowest and default widths next to an open letter. */
const LIST_MIN = 280;
const DIVIDER = 12;

const setExtraWidth = (extra: number, animate = false) =>
  invoke<number>("panel_set_extra_width", { extra, animate });

type PaneMode = "split" | "full";

/**
 * The open letter and where it shows: next to the list ("split", growing the
 * window with an animation when the panel is narrow) or, when the screen has no
 * room, in place of the list ("full"). Closing shrinks the window back first,
 * keeping the split layout until it has, so the list doesn't jump.
 */
function useLetterPane(baseWidth: number) {
  const [letter, setLetter] = useState<Summary | null>(null);
  const [mode, setMode] = useState<PaneMode | null>(null);
  const [closing, setClosing] = useState(false);
  // Refs mirror the state for the async steps; `turn` cancels outdated ones.
  const state = useRef({ mode: null as PaneMode | null, closing: false, grown: false, turn: 0 });
  const update = (patch: Partial<typeof state.current>) => {
    Object.assign(state.current, patch);
    if ("mode" in patch) setMode(patch.mode ?? null);
    if ("closing" in patch) setClosing(!!patch.closing);
  };

  const open = async (next: Summary) => {
    setLetter(next);
    const st = state.current;
    if (st.mode && !st.closing) return; // already open: just another letter
    const turn = ++st.turn;
    update({ mode: "split", closing: false });
    if (baseWidth >= WIDE_PANEL) return;
    const applied = await setExtraWidth(READER_W, true).catch(() => 0);
    if (turn !== state.current.turn) return;
    state.current.grown = applied > 0;
    if (applied < READER_MIN) {
      // No room on screen: the letter takes the list's place instead.
      state.current.grown = false;
      setExtraWidth(0).catch(console.error);
      update({ mode: "full" });
    }
  };

  const close = async () => {
    const st = state.current;
    const turn = ++st.turn;
    if (st.mode === "split" && st.grown) {
      update({ closing: true });
      await setExtraWidth(0, true).catch(console.error);
      if (turn !== state.current.turn) return; // reopened meanwhile
      state.current.grown = false;
    }
    setLetter(null);
    update({ mode: null, closing: false });
  };

  // Leaving the tab gives the width back at once.
  useEffect(() => () => void setExtraWidth(0).catch(console.error), []);

  return { letter, mode, closing, open, close };
}

/** Drag handle between the list and the letter; moves the split, not the window. */
function Divider({ width, max, onChange }: { width: number; max: number; onChange: (w: number, done: boolean) => void }) {
  const drag = useRef<{ x: number; start: number } | null>(null);
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(LIST_MIN, w)));
  return (
    <div
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, start: width };
      }}
      onPointerMove={(e) => drag.current && onChange(clamp(drag.current.start + e.clientX - drag.current.x), false)}
      onPointerUp={(e) => {
        if (!drag.current) return;
        onChange(clamp(drag.current.start + e.clientX - drag.current.x), true);
        drag.current = null;
      }}
      onDoubleClick={() => onChange(-1, true)}
      title="Потяните, чтобы изменить ширину письма. Двойной клик — как было"
      className="group relative shrink-0 cursor-col-resize touch-none"
      style={{ width: DIVIDER }}
    >
      <div className="absolute inset-y-2 left-1/2 w-px -translate-x-1/2 bg-stroke transition-colors group-hover:w-0.5 group-hover:bg-accent/60 group-active:bg-accent" />
    </div>
  );
}

export function MailTab() {
  const queryClient = useQueryClient();
  const setTab = usePanelStore((s) => s.setTab);
  const mailToOpen = usePanelStore((s) => s.mailToOpen);
  const baseWidth = usePanelSettings().width;
  const { data: settings } = useMailSettings();
  const accounts = settings?.accounts ?? [];
  const [account, setAccount] = useState<string | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const pane = useLetterPane(baseWidth);
  const reading = pane.letter;
  const setReading = (m: Summary) => void pane.open(m);
  const savedListWidth = usePrefs((s) => s.mailListWidth);
  const setMailListWidth = usePrefs((s) => s.setMailListWidth);
  const [draftListWidth, setDraftListWidth] = useState<number | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const { messages, errors, isPending, isFetching, isError, error, hasNextPage, fetchNextPage, isFetchingNextPage } =
    useMailList(account, unreadOnly, accounts.length > 0);
  const [cursor, setCursor] = useState(0);
  // A new filter starts at the top.
  useEffect(() => setCursor(0), [account, unreadOnly]);
  // Clicking a letter (or a notification) moves the arrow-key cursor to it.
  useEffect(() => {
    if (!reading) return;
    const i = messages.findIndex((m) => m.account === reading.account && m.uid === reading.uid);
    if (i >= 0) setCursor(i);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the open letter changes
  }, [reading]);

  // ↑/↓ walk the list (and switch the open letter), Enter opens one.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (usePanelStore.getState().query || e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.target instanceof HTMLElement && e.target.closest("input:not([data-panel-search]), textarea")) return;
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && !(e.key === "Enter" && !reading)) return;
      if (messages.length === 0) return;
      e.preventDefault();
      const at = Math.min(cursor, messages.length - 1);
      if (e.key === "Enter") {
        setReading(messages[at]);
        return;
      }
      const next = e.key === "ArrowDown" ? Math.min(at + 1, messages.length - 1) : Math.max(at - 1, 0);
      if (e.key === "ArrowDown" && next >= messages.length - 3 && hasNextPage && !isFetchingNextPage) fetchNextPage();
      setCursor(next);
      if (reading) setReading(messages[next]);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [messages, cursor, reading, hasNextPage, isFetchingNextPage, fetchNextPage]);

  // A clicked notification asks for a letter.
  useEffect(() => {
    if (!mailToOpen) return;
    setReading(mailToOpen);
    usePanelStore.getState().setMailToOpen(null);
  }, [mailToOpen]);

  if (settings && accounts.length === 0) {
    return (
      <div className="flex h-full flex-col">
        <EmptyState icon={Inbox} title="Почта не подключена" text="Добавьте ящик Яндекса, Gmail или другой почты с IMAP в настройках." />
        <button onClick={() => setTab("settings")} className="mx-auto -mt-12 text-[12.5px] text-accent hover:underline">
          Открыть настройки
        </button>
      </div>
    );
  }

  const multi = accounts.length > 1;
  const close = () => void pane.close();
  const reader = reading && (
    <Reader
      key={`${reading.account}/${reading.uid}`}
      summary={reading}
      multiAccount={multi}
      split={pane.mode === "split"}
      onBack={close}
    />
  );
  if (reading && pane.mode === "full") return reader;

  const list = (
    <div className="flex flex-col gap-2 pb-2">
      <div className="flex items-center gap-1.5">
        <div className="flex min-w-0 flex-1 gap-1.5 overflow-x-auto">
          {multi && (
            <>
              <Chip active={!account} onClick={() => setAccount(null)}>
                Все
              </Chip>
              {accounts.map((a) => (
                <Chip key={a.id} active={account === a.id} onClick={() => setAccount(a.id)}>
                  {a.email.split("@")[0]}
                  <span className="text-fg-subtle">@{a.email.split("@")[1]?.split(".")[0]}</span>
                </Chip>
              ))}
            </>
          )}
          <Chip active={unreadOnly} onClick={() => setUnreadOnly(!unreadOnly)}>
            Непрочитанные
          </Chip>
        </div>
        <button
          onClick={() => invalidateMail(queryClient)}
          title="Обновить"
          className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg"
        >
          <RefreshCw className={clsx("size-3.5", isFetching && !isFetchingNextPage && "animate-spin")} />
        </button>
      </div>

      {actionError && (
        <p
          onClick={() => setActionError(null)}
          title="Скрыть"
          className="cursor-default rounded-lg border border-warn/30 bg-warn/5 px-2.5 py-1.5 text-[12px] text-warn"
        >
          {actionError}
        </p>
      )}

      {errors.map(([email, reason]) => (
        <p key={email} className="rounded-lg border border-warn/30 bg-warn/5 px-2.5 py-1.5 text-[12px] text-warn">
          {email}: {reason}
        </p>
      ))}

      {isPending ? (
        <p className="flex items-center gap-2 px-2.5 py-3 text-[12.5px] text-fg-subtle">
          <Loader2 className="size-4 animate-spin" /> Загружаю письма…
        </p>
      ) : isError ? (
        <p className="px-2.5 text-[12.5px] text-warn">{String(error)}</p>
      ) : messages.length === 0 ? (
        <EmptyState
          icon={MailOpen}
          title={unreadOnly ? "Всё прочитано" : "Писем нет"}
          text={unreadOnly ? "Новых писем во входящих нет." : "Во входящих пусто."}
        />
      ) : (
        <div className="flex flex-col">
          {messages.map((m, i) => (
            <Row
              key={`${m.account}/${m.uid}`}
              m={m}
              showAccount={multi && !account}
              active={!!reading && reading.account === m.account && reading.uid === m.uid}
              selected={i === Math.min(cursor, messages.length - 1)}
              onOpen={() => setReading(m)}
              onError={setActionError}
            />
          ))}
          {hasNextPage && (
            <LoadMore loading={isFetchingNextPage} onVisible={() => !isFetchingNextPage && fetchNextPage()} />
          )}
        </div>
      )}
    </div>
  );

  if (!reading || !pane.mode) return list;

  // Sizes against the window as it will be once grown, not as it is mid-animation.
  const content = (baseWidth >= WIDE_PANEL ? baseWidth : baseWidth + READER_W) - 32;
  const maxList = Math.max(LIST_MIN, content - READER_MIN - DIVIDER);
  const defaultList = baseWidth >= WIDE_PANEL ? Math.min(440, Math.round(baseWidth * 0.42)) : baseWidth - 32;
  const listWidth = Math.min(maxList, Math.max(LIST_MIN, draftListWidth ?? savedListWidth ?? defaultList));
  const resize = (w: number, done: boolean) => {
    if (w < 0) {
      // Double click: back to the default split.
      setDraftListWidth(null);
      setMailListWidth(defaultList);
      return;
    }
    setDraftListWidth(done ? null : w);
    if (done) setMailListWidth(w);
  };
  return (
    <div className="flex h-full">
      <div className="scroll-area -mr-1 shrink-0 pr-1" style={{ width: listWidth }}>
        {list}
      </div>
      <Divider width={listWidth} max={maxList} onChange={resize} />
      <motion.div
        className="scroll-area min-w-0 flex-1 pr-1"
        initial={{ opacity: 0, x: 16 }}
        animate={pane.closing ? { opacity: 0, x: 16 } : { opacity: 1, x: 0 }}
        transition={{ duration: 0.2, ease: "easeOut" }}
      >
        {reader}
      </motion.div>
    </div>
  );
}
