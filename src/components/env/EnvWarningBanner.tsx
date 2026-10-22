import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { AlertTriangle, ChevronDown, ChevronUp, X, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { EnvConflict } from "@/types/env";
import { deleteEnvVars } from "@/lib/api/env";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface EnvWarningBannerProps {
  inline?: boolean;
  conflicts: EnvConflict[];
  onDismiss: () => void;
  onDeleted: () => void;
}

export function EnvWarningBanner({
  conflicts,
  onDismiss,
  onDeleted,
  inline = false,
}: EnvWarningBannerProps) {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [selectedConflicts, setSelectedConflicts] = useState<Set<string>>(
    new Set(),
  );
  const [isDeleting, setIsDeleting] = useState(false);
  const [showConfirmDialog, setShowConfirmDialog] = useState(false);

  if (conflicts.length === 0) {
    return null;
  }

  const toggleSelection = (key: string) => {
    const newSelection = new Set(selectedConflicts);
    if (newSelection.has(key)) {
      newSelection.delete(key);
    } else {
      newSelection.add(key);
    }
    setSelectedConflicts(newSelection);
  };

  const toggleSelectAll = () => {
    if (selectedConflicts.size === conflicts.length) {
      setSelectedConflicts(new Set());
    } else {
      setSelectedConflicts(
        new Set(conflicts.map((c) => `${c.varName}:${c.sourcePath}`)),
      );
    }
  };

  const handleDelete = async () => {
    setShowConfirmDialog(false);
    setIsDeleting(true);

    try {
      const conflictsToDelete = conflicts.filter((c) =>
        selectedConflicts.has(`${c.varName}:${c.sourcePath}`),
      );

      if (conflictsToDelete.length === 0) {
        toast.warning(t("env.error.noSelection"));
        return;
      }

      const backupInfo = await deleteEnvVars(conflictsToDelete);

      toast.success(t("env.delete.success"), {
        description: t("env.backup.location", {
          path: backupInfo.backupPath,
        }),
        duration: 5000,
        closeButton: true,
      });

      // 清空选择并通知父组件
      setSelectedConflicts(new Set());
      onDeleted();
    } catch (error) {
      console.error("删除环境变量失败:", error);
      toast.error(t("env.delete.error"), {
        description: String(error),
      });
    } finally {
      setIsDeleting(false);
    }
  };

  const getSourceDescription = (conflict: EnvConflict): string => {
    if (conflict.sourceType === "system") {
      if (conflict.sourcePath.includes("HKEY_CURRENT_USER")) {
        return t("env.source.userRegistry");
      } else if (conflict.sourcePath.includes("HKEY_LOCAL_MACHINE")) {
        return t("env.source.systemRegistry");
      } else {
        return t("env.source.systemEnv");
      }
    } else {
      return conflict.sourcePath;
    }
  };

  return (
    <>
      <div
        data-testid="env-warning-banner"
        className={
          inline
            ? "pointer-events-none w-full max-w-2xl shrink-0"
            : "pointer-events-none fixed inset-x-0 bottom-20 z-[100] px-4"
        }
      >
        <div
          data-testid="env-warning-card"
          className="pointer-events-auto mx-auto w-full max-w-2xl overflow-hidden rounded-xl border border-amber-200/80 bg-amber-50/90 shadow-lg shadow-amber-900/10 backdrop-blur-md animate-slide-up dark:border-amber-900/70 dark:bg-amber-950/80 dark:shadow-black/20"
        >
          <div className="flex items-center gap-3 px-4 py-3 sm:px-5">
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-600 dark:bg-amber-950/60 dark:text-amber-400">
              <AlertTriangle className="h-5 w-5" />
            </div>

            <div className="min-w-0 flex-1">
              <h3 className="truncate text-sm font-semibold text-amber-950 dark:text-amber-50">
                {t("env.warning.title")}
              </h3>
              <p className="mt-0.5 text-xs text-amber-800/80 dark:text-amber-100/75">
                {t("env.warning.description", { count: conflicts.length })}
              </p>
            </div>

            <div className="flex shrink-0 items-center gap-1">
              <Button
                variant="ghost"
                size="sm"
                aria-expanded={isExpanded}
                aria-controls="env-warning-details"
                onClick={() => setIsExpanded(!isExpanded)}
                className="h-8 px-2.5 text-amber-700 hover:bg-amber-50 hover:text-amber-800 dark:text-amber-300 dark:hover:bg-amber-950/60 dark:hover:text-amber-200"
              >
                {isExpanded
                  ? t("env.actions.collapse")
                  : t("env.actions.expand")}
                {isExpanded ? (
                  <ChevronUp className="ml-1 h-4 w-4" />
                ) : (
                  <ChevronDown className="ml-1 h-4 w-4" />
                )}
              </Button>

              <Button
                variant="ghost"
                size="icon"
                aria-label={t("common.close")}
                onClick={onDismiss}
                className="h-8 w-8 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:text-muted-foreground dark:hover:bg-muted dark:hover:text-foreground"
              >
                <X className="h-4 w-4" />
              </Button>
            </div>
          </div>

          <AnimatePresence initial={false}>
            {isExpanded && (
              <motion.div
                id="env-warning-details"
                initial={{ gridTemplateRows: "0fr", opacity: 0 }}
                animate={{ gridTemplateRows: "1fr", opacity: 1 }}
                exit={{ gridTemplateRows: "0fr", opacity: 0 }}
                transition={{
                  gridTemplateRows: {
                    duration: 0.28,
                    ease: [0.32, 0.72, 0, 1],
                  },
                  opacity: { duration: 0.18 },
                }}
                className="grid overflow-hidden"
              >
                <div className="min-h-0 overflow-hidden">
                  <div className="space-y-3 border-t border-amber-200/80 bg-white/45 px-4 py-4 dark:border-amber-900/60 dark:bg-amber-950/30 sm:px-5">
                    <div className="flex items-center gap-2 border-b border-amber-200/70 pb-2 dark:border-amber-900/50">
                      <Checkbox
                        id="select-all"
                        checked={selectedConflicts.size === conflicts.length}
                        onCheckedChange={toggleSelectAll}
                      />
                      <label
                        htmlFor="select-all"
                        className="cursor-pointer text-sm font-medium text-slate-700 dark:text-foreground"
                      >
                        {t("env.actions.selectAll")}
                      </label>
                    </div>

                    <div className="max-h-[min(40vh,20rem)] space-y-2 overflow-y-auto pr-1">
                      {conflicts.map((conflict) => {
                        const key = `${conflict.varName}:${conflict.sourcePath}`;
                        return (
                          <div
                            key={key}
                            className="flex items-start gap-3 rounded-xl border border-border/70 bg-background/80 p-3 dark:bg-card/60"
                          >
                            <Checkbox
                              id={key}
                              checked={selectedConflicts.has(key)}
                              onCheckedChange={() => toggleSelection(key)}
                            />

                            <div className="flex-1 min-w-0">
                              <label
                                htmlFor={key}
                                className="block text-sm font-medium text-foreground cursor-pointer"
                              >
                                {conflict.varName}
                              </label>
                              <p className="text-xs text-muted-foreground mt-1 break-all">
                                {t("env.field.value")}: {conflict.varValue}
                              </p>
                              <p className="text-xs text-muted-foreground mt-1">
                                {t("env.field.source")}:{" "}
                                {getSourceDescription(conflict)}
                              </p>
                            </div>
                          </div>
                        );
                      })}
                    </div>

                    <div className="flex items-center justify-end gap-2 border-t border-amber-200/70 pt-3 dark:border-amber-900/50">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setSelectedConflicts(new Set())}
                        disabled={selectedConflicts.size === 0}
                        className="border-amber-300 text-amber-900 hover:bg-amber-50 dark:border-amber-800 dark:text-amber-200 dark:hover:bg-amber-950/50"
                      >
                        {t("env.actions.clearSelection")}
                      </Button>

                      <Button
                        variant="destructive"
                        size="sm"
                        onClick={() => setShowConfirmDialog(true)}
                        disabled={selectedConflicts.size === 0 || isDeleting}
                        className="gap-1"
                      >
                        <Trash2 className="h-4 w-4" />
                        {isDeleting
                          ? t("env.actions.deleting")
                          : t("env.actions.deleteSelected", {
                              count: selectedConflicts.size,
                            })}
                      </Button>
                    </div>
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      <Dialog open={showConfirmDialog} onOpenChange={setShowConfirmDialog}>
        <DialogContent className="max-w-md" zIndex="top">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("env.confirm.title")}
            </DialogTitle>
            <DialogDescription className="space-y-2">
              <p>
                {t("env.confirm.message", { count: selectedConflicts.size })}
              </p>
              <p className="text-sm text-muted-foreground">
                {t("env.confirm.backupNotice")}
              </p>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setShowConfirmDialog(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleDelete}>
              {t("env.confirm.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
