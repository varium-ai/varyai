import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronUp, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  quotaPanelApi,
  type QuotaControls,
  type QuotaPolicy,
} from "@/lib/api/quotaPanel";

const errors = new Set([
  "quotaPanel.accountChanged",
  "quotaPanel.storage",
  "quotaPanel.loginRequired",
  "quotaPanel.noReset",
  "quotaPanel.pending",
]);
const safeError = (error: unknown) =>
  typeof error === "string" && errors.has(error) ? error : "quotaPanel.failed";
export function CodexQuotaControls({
  account,
  name,
}: {
  account: string;
  name: string;
}) {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const key = ["codex-quota-management", account];
  const query = useQuery({
    queryKey: key,
    queryFn: () => quotaPanelApi.codex(account),
    staleTime: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const [folded, setFolded] = useState(false);
  const [confirmation, setConfirmation] = useState<{
    account: string;
    identity: string;
    credit: string;
    retry: boolean;
  }>();
  const confirm = !!confirmation;
  const [error, setError] = useState<string>();
  const [message, setMessage] = useState<string>();
  const save = useMutation({
    mutationFn: (policy: QuotaPolicy) =>
      quotaPanelApi.policy(account, query.data!.identity, policy),
    onMutate: () => setError(undefined),
    onSuccess: async (policy) => {
      await client.cancelQueries({ queryKey: key });
      client.setQueryData<QuotaControls>(key, (previous) =>
        previous ? { ...previous, policy } : previous,
      );
    },
    onError: (error) => setError(safeError(error)),
  });
  const consume = useMutation({
    mutationFn: () =>
      quotaPanelApi.reset(
        confirmation!.account,
        confirmation!.identity,
        confirmation!.credit,
      ),
    onMutate: () => {
      setError(undefined);
      setMessage(undefined);
    },
    onSuccess: (outcome) => {
      setConfirmation(undefined);
      const code = [
        "reset",
        "nothing_to_reset",
        "no_credit",
        "already_redeemed",
      ].includes(outcome.code)
        ? outcome.code
        : "unknown";
      setMessage(t(`quotaPanel.outcomes.${code}`, { count: outcome.windows }));
      void client.invalidateQueries({ queryKey: key });
      void client.invalidateQueries({
        queryKey: ["codex-provider-quota", account],
      });
    },
    onError: (error) => setError(safeError(error)),
  });
  const data = query.data;
  const busy = save.isPending || consume.isPending || query.isFetching;
  const toggle = (
    label: string,
    checked: boolean,
    patch: Partial<QuotaPolicy>,
    disabled = false,
  ) => (
    <label className="tray-quota-option">
      <span>{t(label)}</span>
      <Switch
        aria-label={t(label)}
        checked={checked}
        disabled={busy || disabled}
        onCheckedChange={() =>
          data && save.mutate({ ...data.policy, ...patch })
        }
      />
    </label>
  );
  if (!data)
    return (
      <p
        className="tray-quota-control-status"
        role={query.isError ? "alert" : "status"}
      >
        {t(
          query.isError
            ? "quotaPanel.controlsFailed"
            : "quotaPanel.controlsLoading",
        )}
      </p>
    );
  const credits = data.policy.credits !== "off";
  return (
    <div className="tray-quota-controls">
      {data.resets.length > 0 && (
        <div className="tray-quota-resets">
          <span>
            <RotateCcw size={12} aria-hidden="true" />{" "}
            {t("quotaPanel.resetCount", { count: data.resets.length })}
            {data.resets[0].until && (
              <>
                {" "}
                ·{" "}
                {t("quotaPanel.until", {
                  time: new Date(data.resets[0].until * 1000).toLocaleString(
                    i18n.language,
                    {
                      month: "short",
                      day: "numeric",
                      hour: "2-digit",
                      minute: "2-digit",
                    },
                  ),
                })}
              </>
            )}
            {data.policy.expiring && (
              <b> · {t("quotaPanel.autoBeforeExpiry")}</b>
            )}
          </span>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setConfirmation({
                account,
                identity: data.identity,
                credit: data.pendingCredit ?? data.resets[0].id,
                retry: data.pending,
              });
              setError(undefined);
            }}
          >
            {t(data.pending ? "quotaPanel.retryReset" : "quotaPanel.useReset")}
          </button>
        </div>
      )}
      <button
        type="button"
        className="tray-quota-fold"
        aria-expanded={!folded}
        onClick={() => setFolded((value) => !value)}
      >
        <span>{t("quotaPanel.autoResets")}</span>
        <ChevronUp
          size={12}
          aria-hidden="true"
          className={folded ? "is-folded" : undefined}
        />
      </button>
      {!folded && (
        <div className="tray-quota-options">
          {toggle("quotaPanel.whenWeekExhausted", data.policy.week, {
            week: !data.policy.week,
          })}
          {toggle("quotaPanel.beforeExpiry", data.policy.expiring, {
            expiring: !data.policy.expiring,
          })}
          {toggle("quotaPanel.useCredits", credits, {
            credits: credits ? "off" : "on",
          })}
          {toggle(
            "quotaPanel.creditsFirst",
            data.policy.credits === "first",
            { credits: data.policy.credits === "first" ? "on" : "first" },
            !credits,
          )}
          {toggle(
            "quotaPanel.creditsLast",
            data.policy.credits === "last",
            { credits: data.policy.credits === "last" ? "on" : "last" },
            !credits,
          )}
        </div>
      )}
      {data.pending && <p role="status">{t("quotaPanel.pending")}</p>}
      {error && !confirm && <p role="alert">{t(error)}</p>}
      {message && <p role="status">{message}</p>}
      <ConfirmDialog
        isOpen={confirm}
        title={`${t("quotaPanel.confirmReset")} · ${name}`}
        message={t(
          confirmation?.retry
            ? "quotaPanel.retryWarning"
            : "quotaPanel.resetWarning",
          { name },
        )}
        confirmText={t("quotaPanel.confirmReset")}
        pending={consume.isPending}
        errorMessage={error ? t(error) : undefined}
        onCancel={() => {
          setConfirmation(undefined);
          setError(undefined);
        }}
        onConfirm={() => consume.mutate()}
      />
      {confirm && data.resets.length > 1 && (
        <span className="sr-only">{t("quotaPanel.earliestReset")}</span>
      )}
    </div>
  );
}
