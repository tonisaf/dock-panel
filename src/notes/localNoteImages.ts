import { convertFileSrc, invoke } from "@tauri-apps/api/core";
export interface NoteImage { id: string; name: string }
export const imageUrl = (image: NoteImage) => convertFileSrc(image.id, "localnoteimg");
export function imageDropTarget(x: number, y: number) {
  return document.elementFromPoint(x / window.devicePixelRatio, y / window.devicePixelRatio)?.closest<HTMLElement>("[data-note-image-target]")?.dataset.noteImageTarget;
}
export async function importImage(source: File | string): Promise<NoteImage> {
  if (typeof source === "string") return invoke("local_note_import_image", { path: source });
  if (source.size > 20 * 1024 * 1024) throw new Error("Изображение превышает 20 МБ");
  const data = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("Не удалось прочитать изображение"));
    reader.onload = () => resolve(String(reader.result).split(",")[1]);
    reader.readAsDataURL(source);
  });
  return invoke("local_note_import_image", { data, name: source.name });
}
export async function importImages(sources: (File | string)[]) {
  if (sources.length > 20) throw new Error("Не больше 20 изображений за один раз");
  const images: NoteImage[] = [];
  for (const source of sources) images.push(await importImage(source));
  return images;
}
export const mergeImages = (existing: NoteImage[], added: NoteImage[]) => {
  const images = [...existing, ...added].filter((image, i, all) => all.findIndex((other) => other.id === image.id) === i);
  if (images.length > 20) throw new Error("Не больше 20 изображений на заметку");
  return images;
};
