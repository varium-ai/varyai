import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Pencil, Trash2, X, RefreshCw } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { profilesApi, type Profile } from "@/lib/api/profiles";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  useDeleteProfileMutation,
  useProfilesQuery,
  useUpdateProfileMutation,
} from "@/lib/query/profiles";

interface ProfileManageDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

type PendingConfirm = {
  id: string;
  name: string;
};

/**
 * Snapshot-based profile management.
 *
 * Renaming and deleting affect the shared profile entity. Multi-agent bundles
 * save only through explicit refresh; legacy single-client profiles keep auto-save.
 */
export function ProfileManageDialog({
  isOpen,
  onClose,
}: ProfileManageDialogProps) {
  const { t } = useTranslation();
  const { data } = useProfilesQuery();
  const updateMutation = useUpdateProfileMutation();
  const deleteMutation = useDeleteProfileMutation();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [confirm, setConfirm] = useState<PendingConfirm | null>(null);
  const queryClient = useQueryClient();
  const [refreshingId, setRefreshingId] = useState<string | null>(null);
  const refreshBundle = async (profile: Profile) => {
    if (!profile.payload.bundle || refreshingId) return;
    setRefreshingId(profile.id);
    try {
      await profilesApi.captureBundle(
        profile.name,
        profile.payload.bundle.selections.map((s) => s.app),
        profile.id,
      );
      toast.success(t("profiles.updateSuccess"));
      await queryClient.invalidateQueries({ queryKey: ["profiles"] });
    } catch {
      toast.error(t("profiles.bundle.operationFailed"));
    } finally {
      setRefreshingId(null);
    }
  };

  const profiles = data?.profiles ?? [];

  const startRename = (id: string, name: string) => {
    setEditingId(id);
    setEditingName(name);
  };

  const cancelRename = () => {
    setEditingId(null);
    setEditingName("");
  };

  const saveRename = () => {
    const name = editingName.trim();
    if (!name || !editingId) return;
    updateMutation.mutate({ id: editingId, name }, { onSuccess: cancelRename });
  };

  const handleConfirm = () => {
    if (!confirm) return;
    deleteMutation.mutate(confirm.id);
    setConfirm(null);
  };

  return (
    <>
      <Dialog
        open={isOpen}
        onOpenChange={(open) => {
          if (!open) {
            cancelRename();
            onClose();
          }
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle>{t("profiles.manageTitle")}</DialogTitle>
            <DialogDescription>
              {t("profiles.manageDescription")}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 max-h-[50vh] space-y-1 overflow-y-auto px-6 pb-4 pt-3">
            {profiles.length === 0 && (
              <div className="py-4 text-center text-sm text-muted-foreground">
                {t("profiles.empty")}
              </div>
            )}
            {profiles.map((profile) => (
              <div
                key={profile.id}
                className="flex min-w-0 items-center gap-1.5 rounded-md px-2 py-1.5 hover:bg-muted/50"
              >
                {editingId === profile.id ? (
                  <>
                    <Input
                      placeholder={t("profiles.namePlaceholder")}
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      className="h-7 min-w-0 flex-1"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter") saveRename();
                        if (e.key === "Escape") cancelRename();
                      }}
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      title={t("common.confirm")}
                      onClick={saveRename}
                      disabled={!editingName.trim() || updateMutation.isPending}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      title={t("common.cancel")}
                      onClick={cancelRename}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </>
                ) : (
                  <>
                    <span
                      className="min-w-0 flex-1 truncate text-sm"
                      title={profile.name}
                    >
                      {profile.name}
                    </span>
                    {profile.payload.bundle && (
                      <TooltipProvider delayDuration={0}>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 shrink-0"
                              aria-label={t("profiles.bundle.recapture")}
                              onClick={() => void refreshBundle(profile)}
                              disabled={refreshingId !== null}
                              aria-busy={refreshingId === profile.id}
                            >
                              <RefreshCw
                                className={`h-3.5 w-3.5 ${refreshingId === profile.id ? "animate-spin" : ""}`}
                              />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("profiles.bundle.recapture")}
                          </TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                    )}
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      title={t("profiles.rename")}
                      onClick={() => startRename(profile.id, profile.name)}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      title={t("profiles.delete")}
                      onClick={() =>
                        setConfirm({
                          id: profile.id,
                          name: profile.name,
                        })
                      }
                    >
                      <Trash2 className="h-3.5 w-3.5 text-destructive" />
                    </Button>
                  </>
                )}
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                cancelRename();
                onClose();
              }}
            >
              {t("common.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {confirm && (
        <ConfirmDialog
          isOpen
          title={t("profiles.deleteConfirmTitle")}
          message={t("profiles.deleteConfirmMessage", { name: confirm.name })}
          variant="destructive"
          onConfirm={handleConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
    </>
  );
}
