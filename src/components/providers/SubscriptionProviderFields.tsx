import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, ExternalLink, Plus, RefreshCw } from "lucide-react";
import { useManagedAuth } from "./forms/hooks/useManagedAuth";
import { type RoutingUpstream } from "@/lib/api/routingUpstreams";
import { copilotGetModelsForAccount } from "@/lib/api/copilot";
import { fetchXaiOauthModels } from "@/lib/api/model-fetch";
import { settingsApi } from "@/lib/api/settings";
import { copyText } from "@/lib/clipboard";
import { Button } from "@/components/ui/button";
import { ProviderModelPreferences } from "./ProviderModelPreferences";

export function SubscriptionProviderFields({
  draft,
  disabled,
  onChange,
  onError,
  onBusyChange,
}: {
  draft: RoutingUpstream;
  disabled: boolean;
  onChange: (value: RoutingUpstream) => void;
  onError: () => void;
  onBusyChange: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  if (draft.source.kind !== "subscription")
    throw new Error("Invalid subscription editor");
  const source = draft.source;
  const auth = useManagedAuth(source.provider);
  const [fetching, setFetching] = useState(false);
  const [catalogFailed, setCatalogFailed] = useState(false);
  const working = fetching || auth.isAddingAccount;
  useEffect(() => {
    onBusyChange(working);
  }, [working, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);
  const locked = disabled || working;
  const models = [
    ...new Map(
      [...source.catalog, ...source.models].map((row) => {
        const value = row as Record<string, unknown>;
        return [String(value.model ?? value.id ?? row), value] as const;
      }),
    ).values(),
  ];
  const selected = new Set(
    source.models.map((row) => String((row as Record<string, unknown>).model)),
  );
  async function discover() {
    if (!source.accountId) return;
    setFetching(true);
    setCatalogFailed(false);
    try {
      const models =
        source.provider === "github_copilot"
          ? (await copilotGetModelsForAccount(source.accountId)).map(
              (model) => ({ model: model.id, displayName: model.name }),
            )
          : (await fetchXaiOauthModels(source.accountId)).map((model) => ({
              model: model.id,
            }));
      onChange({ ...draft, source: { ...source, catalog: models } });
    } catch {
      setCatalogFailed(true);
      onError();
    } finally {
      setFetching(false);
    }
  }
  // A binding is explicit; a changed manager default never chooses an account here.
  return (
    <>
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.accounts")}</span>
        <div className="pv-stack">
          {auth.isStatusError && (
            <p role="alert" className="pv-error">
              {t("subscriptionProvider.statusFailed")}{" "}
              <Button
                type="button"
                variant="ghost"
                disabled={locked}
                onClick={() => void auth.refetchStatus()}
              >
                {t("common.retry")}
              </Button>
            </p>
          )}
          {auth.isLoadingStatus && (
            <p role="status" className="pv-hint">
              {t("common.loading")}
            </p>
          )}
          <div
            className="cx-accounts"
            role="radiogroup"
            aria-label={t("codexProvider.accounts")}
          >
            {auth.accounts.map((account) => (
              <label key={account.id} className="cx-account cx-check">
                <input
                  type="radio"
                  name={`subscription-${draft.id || "new"}`}
                  checked={source.accountId === account.id}
                  disabled={locked || account.requires_reauth}
                  onChange={() => {
                    setCatalogFailed(false);
                    onChange({
                      ...draft,
                      source: {
                        ...source,
                        accountId: account.id,
                        models: [],
                        catalog: [],
                        mode: "vendor",
                      },
                    });
                  }}
                />
                <span>{account.login}</span>
                {account.requires_reauth && (
                  <span className="cx-plan">{t("codexProvider.reauth")}</span>
                )}
              </label>
            ))}
          </div>
          {source.accountId &&
            auth.isStatusSuccess &&
            !auth.accounts.some(
              (account) =>
                account.id === source.accountId && !account.requires_reauth,
            ) && (
              <p role="alert" className="pv-error">
                {t("codexProvider.reauth")}
              </p>
            )}
          <Button
            type="button"
            variant="ghost"
            disabled={locked}
            className="pv-subscription-action"
            onClick={auth.addAccount}
          >
            <Plus size={14} />
            {t("subscriptionProvider.addAccount")}
          </Button>
          {auth.deviceCode && (
            <div className="cx-login">
              <p>{t("subscriptionProvider.deviceHint")}</p>
              <code>{auth.deviceCode.user_code}</code>
              <Button
                type="button"
                variant="ghost"
                aria-label={t("common.copy")}
                onClick={() =>
                  void copyText(auth.deviceCode!.user_code).catch(onError)
                }
              >
                <Copy size={13} />
              </Button>
              <Button
                type="button"
                variant="ghost"
                aria-label={t("subscriptionProvider.openLogin")}
                onClick={() =>
                  void settingsApi
                    .openExternal(auth.deviceCode!.verification_uri)
                    .catch(onError)
                }
              >
                <ExternalLink size={13} />
              </Button>
              <Button type="button" variant="ghost" onClick={auth.cancelAuth}>
                {t("common.cancel")}
              </Button>
            </div>
          )}
          {auth.error && (
            <p role="alert" className="pv-error">
              {t("codexProvider.loginFailed")}
            </p>
          )}
          <p className="pv-hint">{t("subscriptionProvider.accountHint")}</p>
        </div>
      </div>
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.models")}</span>
        <div className="pv-stack">
          <div
            className="pv-protocols"
            role="radiogroup"
            aria-label={t("providerEditor.modelView")}
          >
            {(["vendor", "picked", "best", "usage"] as const).map((mode) => (
              <button
                type="button"
                key={mode}
                role="radio"
                aria-checked={source.mode === mode}
                disabled={locked}
                onClick={() =>
                  onChange({ ...draft, source: { ...source, mode } })
                }
              >
                {t(`providerParity.mode_${mode}`)}
              </button>
            ))}
          </div>
          <p className="pv-hint">{t(`providerParity.hint_${source.mode}`)}</p>
          <Button
            type="button"
            variant="ghost"
            disabled={locked || !source.accountId}
            className="pv-subscription-action"
            onClick={() => void discover()}
          >
            <RefreshCw size={13} />
            {t("codexProvider.fetchModels")}
          </Button>
          {catalogFailed && (
            <p role="status" className="pv-error">
              {t("codexProvider.catalogFailed")}
            </p>
          )}
          {!models.length && (
            <p className="pv-hint">{t("subscriptionProvider.fetchHint")}</p>
          )}
          <div className="pv-model-chips">
            {models.map((row) => {
              const id = String(row.model ?? row.id);
              const checked = selected.has(id);
              return (
                <button
                  type="button"
                  key={id}
                  className={`pv-model-chip ${checked ? "selected" : ""}`}
                  aria-pressed={checked}
                  disabled={locked || (!checked && selected.size >= 256)}
                  onClick={() =>
                    onChange({
                      ...draft,
                      source: {
                        ...source,
                        mode: "picked",
                        models: checked
                          ? source.models.filter(
                              (m) =>
                                String((m as Record<string, unknown>).model) !==
                                id,
                            )
                          : [...source.models, row],
                      },
                    })
                  }
                >
                  {String(row.displayName ?? id)}
                </button>
              );
            })}
          </div>
          {!!source.models.length && (
            <ProviderModelPreferences
              models={source.models}
              disabled={locked}
              onChange={(models) =>
                onChange({ ...draft, source: { ...source, models } })
              }
            />
          )}
          <label className="cx-check">
            <input
              type="checkbox"
              checked={source.groupsOnly}
              disabled={locked}
              onChange={(event) =>
                onChange({
                  ...draft,
                  source: { ...source, groupsOnly: event.target.checked },
                })
              }
            />
            {t("codexProvider.groupsOnly")}
          </label>
        </div>
      </div>
    </>
  );
}
