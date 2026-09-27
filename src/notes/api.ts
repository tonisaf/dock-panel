import { useEffect } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";

export interface NotesSource {
  id: string;
  title: string;
  databaseId: string | null;
}

export interface Tag {
  name: string;
  /** Notion colour name. */
  color: string;
}

export interface Note {
  id: string;
  url: string;
  title: string;
  icon: string | null;
  tags: Tag[];
  pinned: boolean;
  /** ISO. */
  edited: string;
  created: string;
  preview: string;
  /** Made in the panel, not in Notion yet. */
  local: boolean;
}

export interface NotesState {
  configured: boolean;
  source: NotesSource | null;
  /** Pinned first, then the most recently edited. */
  notes: Note[];
  syncedAt: number | null;
  syncing: boolean;
  error: string | null;
  /** Changes waiting for Notion. */
  pending: number;
  /** The database's tag options, and whether it has tags and a pin checkbox. */
  tagOptions: Tag[];
  canTag: boolean;
  canPin: boolean;
}

export interface NoteProps {
  title?: string;
  tags?: string[];
  pinned?: boolean;
}

export interface Span {
  text: string;
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: boolean;
  code?: boolean;
  color?: string;
  href?: string;
}

export interface Block {
  id: string;
  kind: string;
  text?: Span[];
  checked?: boolean;
  note?: string;
  url?: string;
  /** Cached image file. */
  file?: string;
  children?: Block[];
}

export interface NoteHit {
  id: string;
  title: string;
  icon: string | null;
  snippet: string;
}

const KEY = ["notes"];
const pageKey = (id: string) => ["note-page", id];

/** Everything reads the on-disk cache; the backend announces each change. */
export function useNotes() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const un = listen("notes:changed", () => {
      queryClient.invalidateQueries({ queryKey: KEY });
    });
    return () => {
      un.then((f) => f());
    };
  }, [queryClient]);
  return useQuery({ queryKey: KEY, queryFn: () => invoke<NotesState>("notes_state"), staleTime: Infinity });
}

/** A note's blocks; refreshed when a sync lands. */
export function useNotePage(note: Note | null) {
  const queryClient = useQueryClient();
  useEffect(() => {
    const un = listen("notes:changed", () => {
      queryClient.invalidateQueries({ queryKey: ["note-page"] });
    });
    return () => {
      un.then((f) => f());
    };
  }, [queryClient]);
  return useQuery({
    // The edit time is part of the key, so an edited note is read again.
    queryKey: [...pageKey(note?.id ?? ""), note?.edited],
    queryFn: () => invoke<Block[]>("notes_page", { id: note!.id }),
    enabled: !!note,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

export function useNoteSearch(query: string) {
  const term = query.trim();
  return useQuery({
    queryKey: ["notes-search", term],
    queryFn: () => invoke<NoteHit[]>("notes_search", { query: term, limit: 6 }),
    enabled: term.length >= 2,
    staleTime: 10_000,
    placeholderData: keepPreviousData,
  });
}

export function useNotesActions() {
  const queryClient = useQueryClient();
  const put = (s: NotesState) => queryClient.setQueryData(KEY, s);
  return {
    sync: async (force: boolean) => put(await invoke<NotesState>("notes_sync", { force })),
    setSource: async (source: NotesSource | null) => put(await invoke<NotesState>("notes_set_source", { source })),
    create: (title: string, body: string) => invoke<Note>("notes_create", { title, body }),
    /** The note as editable text: "# ", "- ", "1. ", "[ ] ", "> ", indentation for nesting. */
    text: (id: string) => invoke<string>("notes_text", { id }),
    edit: async (note: Note, text: string) => {
      const blocks = await invoke<Block[]>("notes_edit", { id: note.id, text });
      queryClient.setQueryData([...pageKey(note.id), note.edited], blocks);
      await queryClient.invalidateQueries({ queryKey: KEY });
    },
    setProps: async (id: string, props: NoteProps) => {
      await invoke<Note>("notes_set_props", { id, ...props });
      await queryClient.invalidateQueries({ queryKey: KEY });
    },
    toggle: async (note: Note, blockId: string, checked: boolean) => {
      const blocks = await invoke<Block[]>("notes_toggle", { pageId: note.id, blockId, checked });
      queryClient.setQueryData([...pageKey(note.id), note.edited], blocks);
    },
  };
}

/** Whether to add the pin glyph: not when the note's own icon already is a pin. */
export const showPin = (n: Note) => n.pinned && n.icon !== "📌" && n.icon !== "📍";

export const imageSrc = (b: Block) => (b.file ? convertFileSrc(b.file, "noteimg") : b.url);

/** "5 мин назад", "вчера", "12 сент." */
export function ago(iso: string | number) {
  const t = typeof iso === "number" ? iso : Date.parse(iso);
  if (!t) return "";
  const min = Math.round((Date.now() - t) / 60_000);
  if (min < 1) return "только что";
  if (min < 60) return `${min} мин назад`;
  const d = new Date(t);
  const today = new Date();
  const days = Math.round((new Date(today.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86_400_000);
  if (days === 0) return d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  if (days === 1) return "вчера";
  return d.toLocaleDateString("ru-RU", { day: "numeric", month: "short", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
}

/** Notion's colour names as CSS, per theme (see `--notion-*` in index.css). */
export const NOTION_COLORS: Record<string, string> = Object.fromEntries(
  ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"].map((c) => [c, `var(--notion-${c})`]),
);

export function tagStyle(color: string) {
  const c = NOTION_COLORS[color.replace(/_background$/, "")];
  return c ? { color: c, backgroundColor: `color-mix(in srgb, ${c} 16%, transparent)` } : undefined;
}
