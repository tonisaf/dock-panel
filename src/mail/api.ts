import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

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

export interface MailList {
  messages: Summary[];
  /** [email, reason] for accounts that failed. */
  errors: [string, string][];
}

export type MailAction = "read" | "unread" | "archive" | "delete";

const SETTINGS = ["mail-settings"];
const LIST = ["mail-list"];
const UNREAD = ["mail-unread"];

export function useMailSettings() {
  return useQuery({ queryKey: SETTINGS, queryFn: () => invoke<MailSettings>("mail_settings"), staleTime: Infinity });
}

export function useMailList(enabled: boolean) {
  return useQuery({ queryKey: LIST, queryFn: () => invoke<MailList>("mail_list", { account: null }), enabled, staleTime: 30_000 });
}

export function useUnread() {
  return useQuery({
    queryKey: UNREAD,
    queryFn: () => invoke<{ total: number; byAccount: Record<string, number> }>("mail_unread"),
    staleTime: 10_000,
  });
}

/** Everything that changes mail state, keeping the cached list in step. */
export function useMailActions() {
  const queryClient = useQueryClient();
  const patchList = (fn: (m: Summary[]) => Summary[]) =>
    queryClient.setQueryData<MailList>(LIST, (l) => l && { ...l, messages: fn(l.messages) });
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
      const before = queryClient.getQueryData<MailList>(LIST);
      patchList((list) =>
        action === "archive" || action === "delete"
          ? list.filter((x) => !same(x, m))
          : list.map((x) => (same(x, m) ? { ...x, unread: action === "unread" } : x)),
      );
      try {
        await invoke("mail_action", { account: m.account, uid: m.uid, action });
        recount();
      } catch (e) {
        queryClient.setQueryData(LIST, before);
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
export function invalidateMail(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: LIST });
  queryClient.invalidateQueries({ queryKey: UNREAD });
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
