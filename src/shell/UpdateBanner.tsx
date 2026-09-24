import { AnimatePresence, motion } from "motion/react";
import { Download, Loader2 } from "lucide-react";
import { useUpdateActions, useUpdateStatus } from "../lib/updates";

/** Shown at the top of the panel when a newer version is published. */
export function UpdateBanner() {
  const { data } = useUpdateStatus();
  const { install, installing, progress, error } = useUpdateActions();
  const available = data?.available;

  return (
    <AnimatePresence>
      {available && (
        <motion.div
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          className="shrink-0 overflow-hidden"
        >
          <div className="flex items-center gap-2.5 rounded-xl border border-accent/40 bg-accent/12 px-3 py-2 text-[12.5px]">
            <Download className="size-4 shrink-0 text-accent" />
            <span className="min-w-0 flex-1 truncate">
              {error ?? (installing ? `Устанавливаю ${available.version}…` : `Доступна версия ${available.version}`)}
            </span>
            <button
              onClick={install}
              disabled={installing}
              className="flex shrink-0 items-center gap-1.5 rounded-lg bg-accent px-2.5 py-1 font-medium text-on-accent hover:bg-accent/90 disabled:opacity-70"
            >
              {installing && <Loader2 className="size-3.5 animate-spin" />}
              {installing ? (progress != null ? `${progress}%` : "…") : "Обновить"}
            </button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
