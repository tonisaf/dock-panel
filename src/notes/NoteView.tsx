import { useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronRight, ExternalLink, FileText, Link2 } from "lucide-react";
import clsx from "clsx";
import { NOTION_COLORS, imageSrc, type Block, type Span } from "./api";

const open = (href: string) => {
  // Links between Notion pages come as paths.
  const url = href.startsWith("/") ? `https://www.notion.so${href}` : href;
  openUrl(url).catch(console.error);
};

const notionPage = (id: string) => `https://www.notion.so/${id.replace(/-/g, "")}`;

function Text({ spans }: { spans?: Span[] }) {
  if (!spans?.length) return null;
  return (
    <>
      {spans.map((s, i) => {
        const color = s.color && NOTION_COLORS[s.color.replace(/_background$/, "")];
        const bg = s.color?.endsWith("_background");
        const style = color ? (bg ? { backgroundColor: `color-mix(in srgb, ${color} 20%, transparent)` } : { color }) : undefined;
        const className = clsx(
          s.bold && "font-semibold",
          s.italic && "italic",
          (s.strike || s.underline) && [s.strike && "line-through", s.underline && "underline"],
          s.code && "rounded bg-ink/10 px-1 font-mono text-[0.9em]",
        );
        const content = (
          <span key={i} className={className} style={style}>
            {s.text}
          </span>
        );
        return s.href ? (
          <a
            key={i}
            href={s.href}
            onClick={(e) => {
              e.preventDefault();
              open(s.href!);
            }}
            className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
          >
            {content}
          </a>
        ) : (
          content
        );
      })}
    </>
  );
}

function Toggle({ b, onToggle }: { b: Block; onToggle: Props["onToggle"] }) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button onClick={() => setOpen(!open)} className="flex items-start gap-1 text-left">
        <ChevronRight className={clsx("mt-[3px] size-4 shrink-0 text-fg-subtle transition-transform", open && "rotate-90")} />
        <span>
          <Text spans={b.text} />
        </span>
      </button>
      {open && b.children && (
        <div className="mt-1 pl-5">
          <Blocks blocks={b.children} onToggle={onToggle} />
        </div>
      )}
    </div>
  );
}

function Nested({ b, onToggle }: { b: Block; onToggle: Props["onToggle"] }) {
  return b.children?.length ? (
    <div className="mt-1 pl-5">
      <Blocks blocks={b.children} onToggle={onToggle} />
    </div>
  ) : null;
}

function One({ b, index, onToggle }: { b: Block; index: number; onToggle: Props["onToggle"] }): ReactNode {
  switch (b.kind) {
    case "heading_1":
      return <h2 className="mt-3 text-[19px] leading-snug font-semibold"><Text spans={b.text} /></h2>;
    case "heading_2":
      return <h3 className="mt-2.5 text-[16.5px] leading-snug font-semibold"><Text spans={b.text} /></h3>;
    case "heading_3":
      return <h4 className="mt-2 text-[14.5px] leading-snug font-semibold"><Text spans={b.text} /></h4>;
    case "bulleted_list_item":
    case "numbered_list_item":
      return (
        <div>
          <div className="flex gap-2">
            <span className="w-4 shrink-0 text-right text-fg-subtle tabular-nums">{b.kind === "numbered_list_item" ? `${index}.` : "•"}</span>
            <span className="min-w-0 flex-1">
              <Text spans={b.text} />
            </span>
          </div>
          <Nested b={b} onToggle={onToggle} />
        </div>
      );
    case "to_do":
      return (
        <div>
          <label className="flex cursor-pointer items-start gap-2">
            <input
              type="checkbox"
              checked={!!b.checked}
              onChange={(e) => onToggle?.(b.id, e.target.checked)}
              className="mt-[3px] size-4 shrink-0 cursor-pointer accent-[var(--color-accent)]"
            />
            <span className={clsx("min-w-0 flex-1", b.checked && "text-fg-subtle line-through")}>
              <Text spans={b.text} />
            </span>
          </label>
          <Nested b={b} onToggle={onToggle} />
        </div>
      );
    case "toggle":
      return <Toggle b={b} onToggle={onToggle} />;
    case "quote":
      return (
        <blockquote className="border-l-2 border-fg-muted/50 pl-3">
          <Text spans={b.text} />
          <Nested b={b} onToggle={onToggle} />
        </blockquote>
      );
    case "callout":
      return (
        <div className="flex gap-2.5 rounded-lg bg-ink/6 px-3 py-2.5">
          {b.note && <span className="shrink-0">{b.note}</span>}
          <div className="min-w-0 flex-1">
            <Text spans={b.text} />
            <Nested b={b} onToggle={onToggle} />
          </div>
        </div>
      );
    case "code":
      return (
        <pre className="overflow-x-auto rounded-lg bg-ink/8 px-3 py-2.5 font-mono text-[12px] leading-relaxed whitespace-pre">
          {b.text?.map((s) => s.text).join("")}
        </pre>
      );
    case "equation":
      return <div className="overflow-x-auto font-mono text-[12.5px]"><Text spans={b.text} /></div>;
    case "divider":
      return <hr className="my-1 border-stroke" />;
    case "image": {
      const src = imageSrc(b);
      return (
        <figure className="flex flex-col gap-1">
          {src ? <img src={src} alt="" className="max-h-96 max-w-full self-start rounded-lg" /> : null}
          {b.text?.length ? (
            <figcaption className="text-[12px] text-fg-subtle">
              <Text spans={b.text} />
            </figcaption>
          ) : null}
        </figure>
      );
    }
    case "bookmark":
      return b.url ? (
        <button onClick={() => open(b.url!)} className="flex w-full items-center gap-2 rounded-lg border border-stroke px-3 py-2 text-left hover:bg-ink/5">
          <Link2 className="size-4 shrink-0 text-fg-subtle" />
          <span className="min-w-0 flex-1 truncate text-[12.5px] text-accent">{b.text?.length ? b.text.map((s) => s.text).join("") : b.url}</span>
        </button>
      ) : null;
    case "child_page":
      return (
        <button onClick={() => open(notionPage(b.id))} className="flex items-center gap-2 text-left hover:text-accent">
          <FileText className="size-4 shrink-0 text-fg-subtle" />
          <span className="underline decoration-stroke underline-offset-2">
            <Text spans={b.text} />
          </span>
        </button>
      );
    case "unsupported":
      return (
        <p className="text-[12px] text-fg-subtle italic">
          {b.note === "table" ? "Таблица" : `Блок «${b.note}»`} — откройте в Notion
        </p>
      );
    default:
      return (
        <div>
          <p className="min-h-[1.2em] whitespace-pre-wrap">
            <Text spans={b.text} />
          </p>
          <Nested b={b} onToggle={onToggle} />
        </div>
      );
  }
}

interface Props {
  blocks: Block[];
  /** Ticks a to-do; without it the boxes still show but do nothing. */
  onToggle?: (blockId: string, checked: boolean) => void;
}

export function Blocks({ blocks, onToggle }: Props) {
  let n = 0;
  return (
    <div className="flex flex-col gap-1.5">
      {blocks.map((b) => {
        n = b.kind === "numbered_list_item" ? n + 1 : 0;
        return <One key={b.id} b={b} index={n} onToggle={onToggle} />;
      })}
    </div>
  );
}

export function OpenInNotion({ url }: { url: string }) {
  if (!url) return null;
  return (
    <button
      onClick={() => openUrl(url).catch(console.error)}
      title="Открыть в Notion"
      className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg"
    >
      <ExternalLink className="size-3.5" />
    </button>
  );
}
