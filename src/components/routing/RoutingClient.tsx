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
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = !!(status.data?.enabled || status.data?.providerEnabled);
  async function apply(enabled: boolean) {
    setBusy(true);
    setError(null);
    try {
      if (enabled) await intentRoutingClientApi.connect(true);
      else await intentRoutingClientApi.disconnect();
      setConfirm(false);
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
        disabled={disabled || busy || status.isPending || status.isError}
        onCheckedChange={(enabled) =>
          enabled ? setConfirm(true) : void apply(false)
        }
      />
      {status.data?.recoveryPending && (
        <>
          <p role="alert" className="text-xs text-destructive">
            {t("intentRouting.clientRecovery")}
          </p>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy || disabled}
            onClick={() => void apply(false)}
          >
            {t("intentRouting.clientDisconnect")}
          </Button>
        </>
      )}
      {(error || status.isError) && (
        <p role="alert" className="text-xs text-destructive">
          {t(error ?? "intentRouting.clientFailed")}
        </p>
      )}
      {!!status.data?.omittedEntries && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("intentRouting.clientOmitted", {
            count: status.data.omittedEntries,
          })}
        </p>
      )}
      <ConfirmDialog
        isOpen={confirm}
        variant="info"
        title={t("intentRouting.clientUnifiedTitle")}
        message={t("intentRouting.clientUnifiedConsent")}
        confirmText={t("common.enable")}
        pending={busy}
        onConfirm={() => void apply(true)}
        onCancel={() => setConfirm(false)}
      />
    </>
  );
}
