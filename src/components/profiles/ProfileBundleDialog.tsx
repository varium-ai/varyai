import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { profilesApi, type ProfilePreview } from "@/lib/api/profiles";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";

export function ProfileBundleDialog({
  profile,
  onClose,
  onChanged,
}: {
  profile: { id: string; name: string };
  onClose: () => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const restartClients = new Set([
    "claude-desktop",
    "codex",
    "workbuddy",
    "qoder",
    "zcode",
    "trae",
  ]);
  const restartHint = (app: string) =>
    t(
      app === "trae"
        ? "profiles.bundle.clientSyncHint"
        : restartClients.has(app)
          ? "profiles.bundle.restartClient"
          : "profiles.bundle.restartSession",
      { app: t(`apps.${app}`) },
    );
  const [preview, setPreview] = useState<ProfilePreview>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const describeError = (value: unknown) => {
    const message = value instanceof Error ? value.message : String(value);
    const key = message.match(/profiles\.bundle\.[a-zA-Z]+/)?.[0];
    return t(key ?? "profiles.bundle.operationFailed");
  };

  useEffect(() => {
    let active = true;
    profilesApi.previewBundle(profile.id).then(
      (value) => {
        if (active) setPreview(value);
      },
      (value) => {
        if (active) setError(describeError(value));
      },
    );
    return () => {
      active = false;
    };
  }, [profile.id]);

  const apply = async () => {
    if (!preview || busy) return;
    setBusy(true);
    setError("");
    try {
      const result = await profilesApi.applyBundle(
        profile.id,
        preview.revision,
      );
      onChanged();
      if (!result.success) {
        setError(t("profiles.bundle.applyRolledBack"));
        setPreview(undefined);
        return;
      }
      const message = t("profiles.bundle.applied", { name: profile.name });
      const restartHints = preview.changes.map((change) =>
        restartHint(change.app),
      );
      if (result.warnings.length) {
        toast.warning(message, {
          description: [
            ...[...new Set(result.warnings)].map((warning) =>
              t(
                warning.startsWith("profiles.bundle.")
                  ? warning
                  : "profiles.bundle.applyWarning",
              ),
            ),
            ...restartHints,
          ].join("\n"),
          duration: 8000,
        });
      } else {
        toast.success(message, {
          description: restartHints.join("\n"),
          duration: 10000,
          closeButton: true,
        });
      }
      onClose();
    } catch (value) {
      setError(describeError(value));
      setPreview(undefined);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <DialogContent
        className="max-w-xl"
        zIndex="top"
        overlayClassName="backdrop-blur-none"
      >
        <DialogHeader>
          <DialogTitle className="line-clamp-2 break-all" title={profile.name}>
            {t("profiles.bundle.previewTitle", { name: profile.name })}
          </DialogTitle>
          <DialogDescription>
            {t("profiles.bundle.previewDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 max-h-[60vh] space-y-3 overflow-y-auto px-6 py-3">
          {!preview && !error && <p>{t("common.loading")}</p>}
          {preview && (
            <div className="space-y-2 text-sm text-muted-foreground">
              <p>{t("profiles.bundle.scopeHint")}</p>
              <p>{t("profiles.bundle.failurePolicy")}</p>
            </div>
          )}
          {preview?.changes.map((change) => (
            <div
              key={change.app}
              className="rounded-md border p-3 text-sm [overflow-wrap:anywhere]"
            >
              <p className="font-medium">{t(`apps.${change.app}`)}</p>
              <p>
                {change.currentProvider ||
                  t("profiles.bundle.noEnabledProviders")}{" "}
                →{" "}
                <span>
                  {change.targetProvider ||
                    t("profiles.bundle.noEnabledProviders")}
                </span>
              </p>
              <p className="break-words text-muted-foreground">
                {change.currentModels?.join(" · ") ||
                  t("profiles.bundle.defaultModel")}{" "}
                →{" "}
                {change.models.join(" · ") || t("profiles.bundle.defaultModel")}
              </p>
              {!!change.enabledProviders?.length && (
                <p>
                  {t("profiles.bundle.providersLabel")}:{" "}
                  {change.enabledProviders.join(" · ")}
                </p>
              )}
              <p className="mt-2 text-xs text-muted-foreground">
                {restartHint(change.app)}
              </p>
              {change.clientSyncPending && (
                <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">
                  {t("profiles.bundle.clientSyncPending")}
                </p>
              )}
            </div>
          ))}
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>
            {t("common.close")}
          </Button>
          {preview && (
            <Button onClick={apply} disabled={busy}>
              {t(busy ? "profiles.bundle.applying" : "profiles.bundle.apply")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
