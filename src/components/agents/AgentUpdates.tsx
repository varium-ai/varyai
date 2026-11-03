import { Download, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { useAgentUpdates } from "@/hooks/useAgentUpdates";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type Updates = ReturnType<typeof useAgentUpdates>;
export function AgentInstallButton({
  app,
  updates,
  disabled,
  onSelect,
}: {
  app: AppId;
  updates: Updates;
  disabled: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const installing = updates.installing === app;
  return (
    <button
      type="button"
      className="mp-agent-update"
      title={t(
        updates.canInstallCli(app)
          ? "agentInstall.cliHint"
          : "agentInstall.websiteHint",
      )}
      disabled={disabled || updates.busy}
      aria-busy={installing}
      onClick={onSelect}
    >
      {installing ? (
        <Loader2
          size={11}
          className="animate-spin motion-reduce:animate-none"
        />
      ) : (
        <Download size={11} />
      )}
      <span>
        {t(installing ? "agentInstall.installing" : "agentInstall.install")}
      </span>
    </button>
  );
}
export function AgentVersionTag({
  app,
  updates,
  disabled,
}: {
  app: AppId;
  updates: Updates;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const cli = updates.cli.get(app);
  const desktop = updates.desktop?.[app];
  const appVersion = desktop?.source === "desktop" ? desktop.version : null;
  if (!cli?.version && !appVersion) return null;
  const updating = updates.current === app && !updates.installing;
  return (
    <div className="mp-agent-cli">
      {cli?.version && (
        <span className="mp-agent-version">
          {appVersion ? `CLI ${cli.version}` : cli.version}
        </span>
      )}
      {(updates.updatable.includes(app) || updating) && (
        <button
          type="button"
          className="mp-agent-update"
          disabled={disabled || updates.busy}
          aria-busy={updating}
          onClick={() => void updates.start([app])}
        >
          {updating ? (
            <Loader2
              size={11}
              className="animate-spin motion-reduce:animate-none"
            />
          ) : (
            <Download size={11} />
          )}
          <span>
            {t(updating ? "agentUpdates.updating" : "agentUpdates.update", {
              version: cli?.latest_version,
            })}
          </span>
        </button>
      )}
      {appVersion && (
        <span className="mp-agent-version" title={t("agentUpdates.appHint")}>
          App {appVersion}
        </span>
      )}
    </div>
  );
}
export function AgentUpdatesBar({
  updates,
  name,
  disabled,
}: {
  updates: Updates;
  name: (app: AppId) => string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const { results, progress, current } = updates;
  const changes = results
    .map((r) =>
      t(`agentUpdates.${r.outcome}`, {
        app: name(r.app),
        from: r.from,
        to: r.to,
      }),
    )
    .join(" · ");
  const failed = results.some((r) => r.outcome !== "changed");
  const updateBusy =
    updates.busy && !updates.installTarget && !updates.installing;
  const message = updateBusy
    ? current
      ? t("agentUpdates.progress", {
          app: name(current),
          index: progress.index,
          count: progress.total,
        })
      : t(
          updates.pending
            ? "agentUpdates.awaitingConfirmation"
            : "agentUpdates.checking",
        )
    : results.length
      ? `${t("agentUpdates.summary", { ok: results.filter((r) => r.outcome === "changed").length, count: results.length })} ${changes}`
      : t("agentUpdates.available", {
          count: updates.updatable.length,
          names: updates.updatable.map(name).join(", "),
        });
  return (
    <>
      {updates.showBar && (
        <div
          className={`mp-agent-updates${failed ? " failed" : ""}`}
          role="status"
          aria-live="polite"
        >
          <span className="mp-agent-updates-say" title={message}>
            {message}
          </span>
          {(updates.updatable.length >= 2 || updateBusy) && (
            <button
              type="button"
              className="mp-agent-update"
              disabled={disabled || updates.busy}
              aria-busy={updateBusy}
              aria-label={t("agentUpdates.all")}
              onClick={() => void updates.start(updates.updatable)}
            >
              {updateBusy ? (
                <Loader2
                  size={11}
                  className="animate-spin motion-reduce:animate-none"
                />
              ) : (
                <Download size={11} />
              )}
              <span>
                {t(updateBusy ? "agentUpdates.updating" : "agentUpdates.all")}
              </span>
            </button>
          )}
        </div>
      )}
      <Dialog
        open={!!updates.pending}
        onOpenChange={(open) => {
          if (!open) updates.cancel();
        }}
      >
        <DialogContent className="mp-dialog max-w-md">
          <DialogHeader>
            <DialogTitle>{t("agentUpdates.confirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("agentUpdates.confirmHint", {
                names: updates.pending?.ambiguous.map(name).join(", "),
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={updates.cancel}>
              {t("common.cancel")}
            </Button>
            <Button onClick={updates.confirm}>
              {t("agentUpdates.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

export function AgentInstallDialog({
  updates,
  name,
}: {
  updates: Updates;
  name: (app: AppId) => string;
}) {
  const { t } = useTranslation();
  const target = updates.installTarget;
  const cli = target && updates.canInstallCli(target.app);
  return (
    <>
      {updates.installResult && (
        <p
          className={`mp-agent-install-result${["failed", "checkFailed", "unverified"].includes(updates.installResult.outcome) ? " failed" : ""}`}
          role="status"
        >
          {t(`agentInstall.${updates.installResult.outcome}`, {
            app: name(updates.installResult.app),
          })}
        </p>
      )}
      <Dialog
        open={!!target}
        onOpenChange={(open) => {
          if (!open) updates.cancelInstall();
        }}
      >
        <DialogContent className="mp-dialog max-w-md">
          <DialogHeader>
            <DialogTitle>
              {t("agentInstall.title", { app: target ? name(target.app) : "" })}
            </DialogTitle>
            <DialogDescription>
              {t(cli ? "agentInstall.consent" : "agentInstall.websiteHint")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={updates.cancelInstall}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void updates.confirmInstall()}>
              {t(cli ? "agentInstall.confirm" : "agentInstall.openWebsite")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
