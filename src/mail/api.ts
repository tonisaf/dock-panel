import { invoke } from "@tauri-apps/api/core";
import { useInfiniteQuery, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";

export interface Account {
  id: string;
  email: string;
  host: string;
  port: number;
}

export interface MailSettings {
  accounts: Account[];
  notify: boolean;
}

export interface Summary {
  account: string;
  uid: number;
  fromName: string;
  fromEmail: string;
  subject: string;
  /** Unix ms. */
  date: number;
  unread: boolean;
  flagged: boolean;
}

export interface Letter extends Summary {
  to: string;
  text: string;
  attachments: string[];
  webUrl: string;
}

/** Where each account's next (older) page starts; null once it has no more. */
export type Cursors = Record<string, number | null>;

export interface MailList {
  messages: Summary[];
  /** [email, reason] for accounts that failed. */
  errors: [string, string][];
  cursors: Cursors;
  more: boolean;
}

export type MailAction = "read" | "unread" | "archive" | "delete";

const SETTINGS = ["mail-settings"];
const LIST = ["mail-list"];
const UNREAD = ["mail-unread"];

export function useMailSettings() {
  return useQuery({ queryKey: SETTINGS, queryFn: () => invoke<MailSettings>("mail_settings"), staleTime: Infinity });
}

/**
 * The inbox page by page (40 letters per account each), newest first.
 * `account` narrows it to one mailbox, `unread` to unread letters.
 */
export function useMailList(account: string | null, unread: boolean, enabled: boolean) {
  const query = useInfiniteQuery({
    queryKey: [...LIST, account, unread],
    queryFn: ({ pageParam }) => invoke<MailList>("mail_list", { account, cursors: pageParam, unread }),
    initialPageParam: null as Cursors | null,
    getNextPageParam: (last) => (last.more ? last.cursors : undefined),
    enabled,
    // New mail arrives as mail:changed (IMAP IDLE, fallback minute poll, and panel open).
    staleTime: 5 * 60_000,
  });
  const pages = query.data?.pages ?? [];
  // A letter can show up on two pages when the inbox shifts in between.
  const seen = new Set<string>();
  const all = pages
    .flatMap((p) => p.messages)
    .filter((m) => {
      const key = `${m.account}/${m.uid}`;
      return !seen.has(key) && !!seen.add(key);
    })
    .sort((a, b) => b.date - a.date);
  // Each account pages on its own, so a quiet mailbox's 40 letters can reach
  // much further back than a busy one's. Show only down to where every account
  // with more to load has been loaded, or the busy one's older letters would
  // later pop up in the middle of the list.
  const cursors = pages[pages.length - 1]?.cursors ?? {};
  let frontier = -Infinity;
  const oldest = new Map<string, number>();
  for (const m of all) oldest.set(m.account, m.date); // sorted newest first: the last one wins
  for (const [id, cursor] of Object.entries(cursors)) {
    const date = oldest.get(id);
    if (cursor != null && date !== undefined) frontier = Math.max(frontier, date);
  }
  const messages = all.filter((m) => m.date >= frontier);
  return { ...query, messages, errors: pages[0]?.errors ?? [] };
}

export function useUnread() {
  return useQuery({
    queryKey: UNREAD,
    queryFn: () => invoke<{ total: number; byAccount: Record<string, number> }>("mail_unread"),
    staleTime: 5 * 60_000,
  });
}

/** Everything that changes mail state, keeping the cached list in step. */
export function useMailActions() {
  const queryClient = useQueryClient();
  // Every cached list (all filters), every loaded page.
  const patchList = (fn: (m: Summary[]) => Summary[]) =>
    queryClient.setQueriesData<InfiniteData<MailList>>(
      { queryKey: LIST },
      (d) => d && { ...d, pages: d.pages.map((p) => ({ ...p, messages: fn(p.messages) })) },
    );
  const same = (a: Summary, b: { account: string; uid: number }) => a.account === b.account && a.uid === b.uid;
  // The poller recounts unread mail and emits `mail:changed`.
  const recount = () => invoke("mail_refresh").catch(console.error);
  const reloadSettings = () => queryClient.invalidateQueries({ queryKey: SETTINGS });

  return {
    open: async (m: Summary) => {
      const letter = await invoke<Letter>("mail_open", { account: m.account, uid: m.uid });
      if (m.unread) {
        patchList((list) => list.map((x) => (same(x, m) ? { ...x, unread: false } : x)));
        recount();
      }
      return letter;
    },

    act: async (m: Summary, action: MailAction) => {
      const before = queryClient.getQueriesData<InfiniteData<MailList>>({ queryKey: LIST });
      patchList((list) =>
        action === "archive" || action === "delete"
          ? list.filter((x) => !same(x, m))
          : list.map((x) => (same(x, m) ? { ...x, unread: action === "unread" } : x)),
      );
      try {
        await invoke("mail_action", { account: m.account, uid: m.uid, action });
        recount();
      } catch (e) {
        before.forEach(([key, data]) => queryClient.setQueryData(key, data));
        throw e;
      }
    },

    add: async (email: string, password: string, host: string | null) => {
      await invoke<Account>("mail_add", { email, password, host });
      await reloadSettings();
      recount();
    },

    remove: async (id: string) => {
      await invoke("mail_remove", { id });
      await reloadSettings();
      queryClient.invalidateQueries({ queryKey: LIST });
    },

    setNotify: async (on: boolean) => {
      await invoke("mail_set_notify", { on });
      await reloadSettings();
    },
  };
}

/** Re-reads the list and counts; call on `mail:changed`. */
export function invalidateMail(queryClient: ReturnType<typeof useQueryClient>, refetchType: "active" | "none" = "active") {
  queryClient.invalidateQueries({ queryKey: LIST, refetchType });
  queryClient.invalidateQueries({ queryKey: UNREAD, refetchType });
}

/** "14:05" today, "вчера", "25 сент." this year, "25.09.2025" before. */
export function shortDate(ms: number, now = new Date()) {
  const d = new Date(ms);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86_400_000);
  if (diff === 0) return d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  if (diff === 1) return "вчера";
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
  return d.toLocaleDateString("ru-RU");
}
