import { imageDropTarget } from "../notes/localNoteImages";
import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Pin } from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FILE_PREFIX } from "../lib/apps";
import { usePrefs } from "../lib/prefs";

/** Files and folders dropped from Explorer anywhere on the panel get pinned. */
export function DropToPin() {
  const [over, setOver] = useState(false);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type !== "leave" && imageDropTarget(payload.position.x, payload.position.y)) { setOver(false); return; }
      if (payload.type === "enter" || payload.type === "over") setOver(true);
      else if (payload.type === "leave") setOver(false);
      else {
        setOver(false);
        usePrefs.getState().pinMany(payload.paths.map((p) => FILE_PREFIX + p));
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <AnimatePresence>
      {over && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          className="pointer-events-none absolute inset-2 z-40 grid place-items-center rounded-2xl border-2 border-dashed border-accent bg-accent/10 backdrop-blur-sm"
        >
          <div className="flex flex-col items-center gap-2 text-center">
            <Pin className="size-6 text-accent" />
            <span className="text-[14px] font-medium">Отпустите, чтобы закрепить</span>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
