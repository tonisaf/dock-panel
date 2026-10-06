import { expect, it } from "vitest";
import { mergeImages } from "./localNoteImages";
it("does not duplicate an image pasted or dropped twice", () => {
  const image = { id: "same.png", name: "Clipboard" };
  expect(mergeImages([image], [{ ...image, name: "Screenshot.png" }])).toEqual([image]);
  expect(() => mergeImages(Array.from({ length: 20 }, (_, i) => ({ id: `${i}.png`, name: "" })), [{ id: "extra.png", name: "" }])).toThrow("20");
});
