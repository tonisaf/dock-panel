export interface ChecklistItem { id: string; text: string; checked: boolean }

export function textToChecklist(body: string, newId: () => string): ChecklistItem[] {
  return body.split(/\r?\n/).filter((line) => line.trim()).map((line) => {
    const match = line.match(/^\s*(?:-\s+)?\[([ xX])\]\s+(.*)$/);
    return { id: newId(), text: match ? match[2] : line, checked: !!match && match[1] !== " " };
  });
}

export function checklistToText(items: ChecklistItem[]) {
  return items.map((item) => `- [${item.checked ? "x" : " "}] ${item.text}`).join("\n");
}
