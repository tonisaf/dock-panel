import type { ReactNode } from "react";

/** `code` and **bold** inside a line. */
function inline(text: string): ReactNode[] {
  return text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
      return (
        <code key={i} className="rounded bg-ink/10 px-1 py-px font-mono text-[0.92em]">
          {part.slice(1, -1)}
        </code>
      );
    }
    if (part.startsWith("**") && part.endsWith("**") && part.length > 3) {
      return <strong key={i}>{part.slice(2, -2)}</strong>;
    }
    return part;
  });
}

/**
 * Just enough Markdown for chat answers: fenced code, headings, bullet and
 * numbered lists, paragraphs, inline code and bold. Unclosed fences (while an
 * answer is still streaming) render as code so far.
 */
export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const parts = text.split(/```/);
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const newline = part.indexOf("\n");
      const code = newline >= 0 ? part.slice(newline + 1) : part;
      blocks.push(
        <pre
          key={`c${i}`}
          className="overflow-x-auto rounded-lg bg-ink/8 px-2.5 py-2 font-mono text-[12px] leading-relaxed whitespace-pre"
        >
          {code.replace(/\n$/, "")}
        </pre>,
      );
      return;
    }
    let list: { ordered: boolean; items: string[] } | null = null;
    const flush = (key: string) => {
      if (!list) return;
      const Tag = list.ordered ? "ol" : "ul";
      blocks.push(
        <Tag key={key} className={list.ordered ? "list-decimal pl-5" : "list-disc pl-5"}>
          {list.items.map((item, j) => (
            <li key={j}>{inline(item)}</li>
          ))}
        </Tag>,
      );
      list = null;
    };
    part.split("\n").forEach((line, j) => {
      const key = `t${i}-${j}`;
      const bullet = line.match(/^\s*[-*•]\s+(.*)$/);
      const numbered = line.match(/^\s*\d+[.)]\s+(.*)$/);
      if (bullet || numbered) {
        const ordered = !!numbered;
        if (list && list.ordered !== ordered) flush(`${key}l`);
        list ??= { ordered, items: [] };
        list.items.push((bullet ?? numbered)![1]);
        return;
      }
      flush(`${key}l`);
      const heading = line.match(/^#{1,4}\s+(.*)$/);
      if (heading) blocks.push(<p key={key} className="font-semibold">{inline(heading[1])}</p>);
      else if (line.trim()) blocks.push(<p key={key}>{inline(line)}</p>);
    });
    flush(`t${i}-end`);
  });
  return <div className="flex flex-col gap-1.5 text-[13px] leading-relaxed select-text">{blocks}</div>;
}
