import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { proxyKeys } from "@/lib/query/proxy";
import { intentRoutingClientApi } from "@/lib/api/intentRouting";

export function RoutingClient({ disabled = false }: { disabled?: boolean }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const status = useQuery({
    queryKey: ["intent-routing-client"],
    queryFn: intentRoutingClientApi.status,
    refetchInterval: 10000,
  });
  const [confirm, setConfirm] = useState<"enable" | "cleanup" | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = !!(status.data?.enabled || status.data?.providerEnabled);
  const native = status.data?.nativeCompatibility;
  const nativeMessage = native
    ? {
        disabled: null,
        pending: "intentRouting.nativePending",
        active: "intentRouting.nativeActive",
        unsupported: "intentRouting.nativeUnsupported",
        revoked: "intentRouting.nativeRevoked",
        reconnect_required: "intentRouting.nativeRevoked",
        recovery_pending: "intentRouting.nativeRecovery",
      }[native]
    : null;
  const showError =
    error &&
    !(error === "intentRouting.clientConflict" && status.data?.recoveryPending);
  async function apply(action: "enable" | "disable" | "cleanup") {
    setBusy(true);
    setError(null);
    try {
      if (action === "enable") await intentRoutingClientApi.connect(true);
      else if (action === "cleanup") await intentRoutingClientApi.cleanup();
      else await intentRoutingClientApi.disconnect();
    } catch (cause) {
      const code =
        typeof cause === "string" ? cause.replace(/^intentRouting\./, "") : "";
      setError(
        [
          "clientConflict",
          "proxyRequired",
          "providerUnavailable",
          "modeRecoveryRequired",
        ].includes(code)
          ? `intentRouting.${code}`
          : "intentRouting.clientFailed",
      );
    } finally {
      setConfirm(null);
      await Promise.all([
        client.invalidateQueries({ queryKey: ["intent-routing-client"] }),
        client.invalidateQueries({ queryKey: proxyKeys.status }),
        client.invalidateQueries({ queryKey: proxyKeys.takeoverStatus }),
      ]);
      setBusy(false);
    }
  }
  return (
    <>
      <span className="text-sm font-medium">Codex</span>
      <Switch
        aria-label="Codex"
        checked={active}
        disabled={
          disabled ||
          busy ||
          status.isPending ||
          status.isError ||
          (status.data?.recoveryPending && !status.data.orphanedConfig)
        }
        onCheckedChange={(enabled) =>
          enabled ? setConfirm("enable") : void apply("disable")
        }
      />
      {status.data?.recoveryPending && (
        <>
          <p
            role={status.data.orphanedConfig ? "status" : "alert"}
            className={
              status.data.orphanedConfig
                ? "text-xs text-muted-foreground"
                : "text-xs text-destructive"
            }
          >
            {t(
              status.data.orphanedConfig
                ? status.data.orphanCleanupAvailable
                  ? "intentRouting.clientOrphanRecovery"
                  : "intentRouting.clientOrphanUnsafe"
                : "intentRouting.clientRecovery",
            )}
          </p>
          {(!status.data.orphanedConfig ||
            status.data.orphanCleanupAvailable) && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || disabled}
              onClick={() =>
                status.data.orphanedConfig
                  ? setConfirm("cleanup")
                  : void apply("disable")
              }
            >
              {t(
                status.data.orphanedConfig
                  ? "intentRouting.clientCleanup"
                  : "intentRouting.clientDisconnect",
              )}
            </Button>
          )}
        </>
      )}
      {(showError || status.isError) && (
        <p role="alert" className="text-xs text-destructive">
          {t(error ?? "intentRouting.clientFailed")}
        </p>
      )}
      {status.data?.enabled && !status.data.recoveryPending && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("intentRouting.clientRestart")}
        </p>
      )}
      {active && nativeMessage && (
        <p role="status" className="text-xs text-muted-foreground">
          {t(nativeMessage)}
        </p>
      )}
      {active &&
        !status.data?.recoveryPending &&
        (native === "revoked" || native === "reconnect_required") && (
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy || disabled}
            onClick={() => setConfirm("enable")}
          >
            {t("intentRouting.nativeReconnect")}
          </Button>
        )}
      {!!status.data?.omittedEntries && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("intentRouting.clientOmitted", {
            count: status.data.omittedEntries,
          })}
        </p>
      )}
      <ConfirmDialog
        isOpen={confirm !== null}
        variant="info"
        title={t(
          confirm === "cleanup"
            ? "intentRouting.clientCleanupTitle"
            : "intentRouting.clientUnifiedTitle",
        )}
        message={
          confirm === "cleanup"
            ? t("intentRouting.clientCleanupConsent")
            : `${t("intentRouting.clientUnifiedConsent")} ${t("intentRouting.nativeConsent")}${
                status.data?.orphanedConfig
                  ? ` ${t("intentRouting.clientOrphanReconnectConsent")}`
                  : ""
              }`
        }
        confirmText={t(
          confirm === "cleanup"
            ? "intentRouting.clientCleanup"
            : "common.enable",
        )}
        pending={busy}
        onConfirm={() => confirm && void apply(confirm)}
        onCancel={() => setConfirm(null)}
      />
    </>
  );
}
