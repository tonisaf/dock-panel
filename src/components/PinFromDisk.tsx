import { FilePlus, FolderPlus } from "lucide-react";
import { pinFromDisk } from "../lib/apps";

const button = "grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg";

/** "Pin files" / "Pin folders" buttons for the pinned sections' headers. */
export function PinFromDisk() {
  return (
    <>
      <button className={button} title="Закрепить файлы…" onClick={() => pinFromDisk(false)}>
        <FilePlus className="size-3.5" />
      </button>
      <button className={button} title="Закрепить папки…" onClick={() => pinFromDisk(true)}>
        <FolderPlus className="size-3.5" />
      </button>
    </>
  );
}
