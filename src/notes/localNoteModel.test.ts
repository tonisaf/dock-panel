import { expect, it } from "vitest";
import { checklistToText, textToChecklist } from "./localNoteModel";

it("converts plain text and Markdown without losing checked state", () => {
  let n = 0;
  const items = textToChecklist("Milk\r\n\r\n- [X] Bread\n[ ] Call", () => String(++n));
  expect(items).toEqual([{ id: "1", text: "Milk", checked: false }, { id: "2", text: "Bread", checked: true }, { id: "3", text: "Call", checked: false }]);
  expect(textToChecklist(checklistToText(items), () => "id").map(({ text, checked }) => ({ text, checked }))).toEqual(items.map(({ text, checked }) => ({ text, checked })));
});
