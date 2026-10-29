import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Profile, ProfileScope } from "@/lib/api/profiles";
import { providersApi } from "@/lib/api/providers";
import { useApplyProfileMutation } from "@/lib/query/profiles";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { hasScopeSnapshot } from "./scope";

export function ProfileLegacyDialog({
  profile,
  scope,
  onClose,
}: {
  profile: Profile;
  scope: ProfileScope;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const apply = useApplyProfileMutation();
  const { data: providers } = useQuery({
    queryKey: ["profile-provider-names", scope],
    queryFn: () => providersApi.getAll(scope),
  });
  const providerId = profile.payload.providers[scope];
  const rows = [
    [
      t("profiles.legacyProvider"),
      providerId
        ? (providers?.[providerId]?.name ?? providerId)
        : t("profiles.unchanged"),
    ],
    [
      t("profiles.bundle.mcpLabel"),
      profile.payload.mcp[scope] === null
        ? t("profiles.unchanged")
        : profile.payload.mcp[scope]?.join(" · ") || t("profiles.noneSelected"),
    ],
    [
      t("profiles.bundle.skillsLabel"),
      profile.payload.skills[scope] === null
        ? t("profiles.unchanged")
        : profile.payload.skills[scope]?.join(" · ") ||
          t("profiles.noneSelected"),
    ],
    [
      t("profiles.bundle.promptsLabel"),
      profile.payload.prompts[scope] || t("profiles.unchanged"),
    ],
  ];
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !apply.isPending) onClose();
      }}
    >
      <DialogContent className="max-w-xl" zIndex="top">
        <DialogHeader>
          <DialogTitle className="line-clamp-2 break-all" title={profile.name}>
            {t("profiles.bundle.previewTitle", { name: profile.name })}
          </DialogTitle>
          <DialogDescription>
            {t("profiles.legacyDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 max-h-[60vh] space-y-3 overflow-y-auto px-6 py-3 text-sm [overflow-wrap:anywhere]">
          <p className="font-medium">{t(`apps.${scope}`)}</p>
          {hasScopeSnapshot(profile, scope) ? (
            <dl className="space-y-3">
              {rows.map(([label, value]) => (
                <div key={label}>
                  <dt className="text-muted-foreground">{label}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
          ) : (
            <p>{t("profiles.noSnapshotForScope")}</p>
          )}
          <p className="text-muted-foreground">
            {t(
              scope === "claude"
                ? "profiles.bundle.restartSession"
                : "profiles.bundle.restartClient",
              { app: t(`apps.${scope}`) },
            )}
          </p>
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={onClose}
            disabled={apply.isPending}
          >
            {t("common.close")}
          </Button>
          <Button
            disabled={apply.isPending}
            onClick={() =>
              apply.mutate({ id: profile.id, scope }, { onSuccess: onClose })
            }
          >
            {t(
              apply.isPending
                ? "profiles.bundle.applying"
                : "profiles.bundle.apply",
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
