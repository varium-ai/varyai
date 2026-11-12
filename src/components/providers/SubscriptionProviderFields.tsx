import { useCallback, useEffect, useState, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Copy, ExternalLink } from "lucide-react";
import { useManagedAuth } from "./forms/hooks/useManagedAuth";
import { type RoutingUpstream } from "@/lib/api/routingUpstreams";
import { copilotGetModelsForAccount } from "@/lib/api/copilot";
import { fetchXaiOauthModels } from "@/lib/api/model-fetch";
import { settingsApi } from "@/lib/api/settings";
import { copyText } from "@/lib/clipboard";
import { Button } from "@/components/ui/button";
import { isImageGenerationModel } from "@/lib/providerModelKind";
import { ProviderImageModels } from "./ProviderImageModels";

import { Input } from "@/components/ui/input";
import { SubscriptionAccountQuota } from "./SubscriptionAccountQuota";
import { NativeSubscriptionAccounts } from "./NativeSubscriptionAccounts";
import { SubscriptionAccountActions } from "./SubscriptionAccountActions";
import type { ProviderErrorHandler } from "./providerErrorToast";
import { SubscriptionModelChip } from "./SubscriptionModelChip";
import {
  nativeSubscriptionApi,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";

type Props = {
  draft: RoutingUpstream;
  disabled: boolean;
  editorOpen?: boolean;
  startReauth?: boolean;
  onReauthStarted?: () => void;
  onChange: (value: RoutingUpstream) => void;
  onError: ProviderErrorHandler;
  onBusyChange: (value: boolean) => void;
};
export function SubscriptionProviderFields(props: Props) {
  const source = props.draft.source;
  if (source.kind !== "subscription")
    throw new Error("Invalid subscription editor");
  if (source.provider === "github_copilot" || source.provider === "xai_oauth")
    return <ManagedSubscriptionFields {...props} provider={source.provider} />;
  return <SubscriptionFields {...props} />;
}
function ManagedSubscriptionFields(
  props: Props & { provider: "github_copilot" | "xai_oauth" },
) {
  const auth = useManagedAuth(props.provider);
  return <SubscriptionFields {...props} auth={auth} />;
}
function SubscriptionFields({
  draft,
  disabled,
  editorOpen = true,
  startReauth = false,
  onReauthStarted,
  onChange,
  onError,
  onBusyChange,
  auth,
}: Props & { auth?: ReturnType<typeof useManagedAuth> }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  if (draft.source.kind !== "subscription")
    throw new Error("Invalid subscription editor");
  const source = draft.source;
  useEffect(() => {
    if (!editorOpen) auth?.cancelAuth();
  }, [editorOpen, auth?.cancelAuth]);
  const [nativeBusy, setNativeBusy] = useState(false);
  const nativeBusyChange = useCallback(
    (busy: boolean) => setNativeBusy(busy),
    [],
  );
  const [fetching, setFetching] = useState(false);
  const [filter, setFilter] = useState("");
  const working = fetching || nativeBusy || !!auth?.isRemovingAccount;
  useEffect(() => {
    onBusyChange(working);
  }, [working, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);
  const locked = disabled || working;
  const authenticating = !!auth?.isAddingAccount;
  const removedAccount = useRef<string>();
  const loginTarget = useRef<string>();
  const autoReauthed = useRef(false);
  useEffect(() => {
    if (!startReauth) {
      autoReauthed.current = false;
      return;
    }
    if (
      !editorOpen ||
      autoReauthed.current ||
      locked ||
      authenticating ||
      !auth
    )
      return;
    autoReauthed.current = true;
    onReauthStarted?.();
    handledLogin.current = auth.loginCompletion?.generation;
    loginTarget.current = source.accountId || undefined;
    auth.addAccount();
  }, [
    startReauth,
    editorOpen,
    locked,
    authenticating,
    auth,
    source.accountId,
    onReauthStarted,
  ]);
  useEffect(() => {
    const id = removedAccount.current;
    if (
      !id ||
      !auth?.isStatusSuccess ||
      auth.isRemovingAccount ||
      auth.accounts.some((account) => account.id === id)
    )
      return;
    removedAccount.current = undefined;
    if (id === source.accountId)
      onChange({ ...draft, source: { ...source, accountId: "" } });
    void client.invalidateQueries({ queryKey: ["routing-upstreams"] });
  }, [
    auth?.accounts,
    auth?.isRemovingAccount,
    auth?.isStatusSuccess,
    source.accountId,
    client,
    draft,
    onChange,
  ]);
  const handledLogin = useRef<number>();
  useEffect(() => {
    const completion = auth?.loginCompletion;
    if (!completion || handledLogin.current === completion.generation) return;
    handledLogin.current = completion.generation;
    if (!loginTarget.current) return;
    if (completion.accountId !== loginTarget.current) {
      // A different identity must be explicitly selected; never redirect an existing binding.
      onError("signIn");
    }
    const repaired = completion.accountId === loginTarget.current;
    loginTarget.current = undefined;
    if (repaired && completion.accountId === source.accountId) void discover();
    void client.resetQueries({
      queryKey: [
        "subscription-account-quota",
        source.provider,
        completion.accountId,
      ],
    });
    void client.invalidateQueries({ queryKey: ["routing-upstreams"] });
    void client.invalidateQueries({ queryKey: ["agent-model-catalog"] });
  }, [auth?.loginCompletion, client, onError]);
  const available = new Map<string, Record<string, unknown>>();
  for (const row of [...source.models, ...source.catalog]) {
    const value = row as Record<string, unknown>;
    const id = String(value.model ?? value.id ?? row);
    available.set(id, { ...available.get(id), ...value });
  }
  const imageModels = [...available.values()].filter(isImageGenerationModel);
  const models = [...available.values()].filter(
    (row) => !isImageGenerationModel(row),
  );
  const term = filter.trim().toLocaleLowerCase();
  const filtered = models.filter((row) =>
    `${row.model ?? row.id} ${(row.modelPreferences as { name?: string })?.name ?? row.displayName ?? ""}`
      .toLocaleLowerCase()
      .includes(term),
  );
  const imagePicks = source.models.filter(isImageGenerationModel);
  function toggleImage(id: string) {
    onChange({
      ...draft,
      source: {
        ...source,
        models: selected.has(id)
          ? source.models.filter(
              (row) => String((row as Record<string, unknown>).model) !== id,
            )
          : [...source.models, available.get(id)!],
      },
    });
  }
  const selected = new Set(
    source.models.map((row) => String((row as Record<string, unknown>).model)),
  );
  const latest = useRef({ draft, onChange, onError, editorOpen });
  latest.current = { draft, onChange, onError, editorOpen };
  const discoveryGeneration = useRef(0);
  useEffect(() => {
    setFetching(false);
    if (editorOpen && source.accountId) void discover(false);
    return () => {
      discoveryGeneration.current += 1;
    };
  }, [source.provider, source.accountId, editorOpen]);
  async function discover(force = true) {
    if (!source.accountId || !editorOpen) return;
    const generation = ++discoveryGeneration.current;
    const current = () => {
      const next = latest.current.draft.source;
      return (
        generation === discoveryGeneration.current &&
        latest.current.editorOpen &&
        next.kind === "subscription" &&
        next.provider === source.provider &&
        next.accountId === source.accountId
      );
    };
    setFetching(true);
    try {
      const models = await client.fetchQuery({
        queryKey: [
          "subscription-account-models",
          source.provider,
          source.accountId,
        ],
        staleTime: force ? 0 : 60_000,
        queryFn: async () =>
          source.provider === "github_copilot"
            ? (await copilotGetModelsForAccount(source.accountId)).map(
                (model) => ({
                  ...model,
                  model: model.id,
                  displayName: model.name,
                }),
              )
            : source.provider === "xai_oauth"
              ? (await fetchXaiOauthModels(source.accountId)).map((model) => ({
                  ...model,
                  model: model.id,
                }))
              : await nativeSubscriptionApi.models(
                  source.provider as NativeSubscriptionProvider,
                  source.accountId,
                ),
      });
      if (current()) {
        const next = latest.current.draft;
        if (next.source.kind === "subscription")
          latest.current.onChange({
            ...next,
            source: { ...next.source, catalog: models },
          });
      }
    } catch (cause) {
      if (current()) {
        void client.invalidateQueries({
          queryKey: ["managed-auth-status", source.provider],
        });
        void client.invalidateQueries({
          queryKey: ["native-subscription-accounts", source.provider],
        });
        void client.invalidateQueries({ queryKey: ["routing-upstreams"] });
        latest.current.onError("fetchModels", cause);
      }
    } finally {
      if (generation === discoveryGeneration.current) setFetching(false);
    }
  }
  // A binding is explicit; a changed manager default never chooses an account here.
  return (
    <>
      {!auth ? (
        <NativeSubscriptionAccounts
          editorOpen={editorOpen}
          provider={source.provider as NativeSubscriptionProvider}
          accountId={source.accountId}
          disabled={disabled || fetching}
          startReauth={startReauth}
          onReauthStarted={onReauthStarted}
          onBusyChange={nativeBusyChange}
          onError={() => onError("account")}
          onReauthenticated={(id) => {
            if (id === source.accountId) void discover();
          }}
          onSelect={(accountId) => {
            onChange({
              ...draft,
              source:
                accountId === source.accountId
                  ? source
                  : {
                      ...source,
                      accountId,
                      models: [],
                      catalog: [],
                      mode: "vendor",
                    },
            });
          }}
        />
      ) : (
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
                <div key={account.id} className="cx-account">
                  <div className="cx-account-name">
                    <label className="cx-check">
                      <input
                        type="radio"
                        name={`subscription-${draft.id || "new"}`}
                        checked={source.accountId === account.id}
                        disabled={
                          locked ||
                          account.requires_reauth ||
                          account.reauth_required
                        }
                        onChange={() => {
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
                    </label>
                    {account.plan && (
                      <span className="cx-plan">{account.plan}</span>
                    )}
                    {source.accountId === account.id && (
                      <span className="cx-using">{t("provider.inUse")}</span>
                    )}
                    {(account.requires_reauth || account.reauth_required) && (
                      <>
                        <span className="cx-plan cx-reauth">
                          {t("codexOauth.reauthBadge")}
                        </span>
                        <Button
                          type="button"
                          variant="ghost"
                          disabled={locked}
                          aria-label={`${t("codexOauth.reauthLogin")} ${account.login}`}
                          onClick={() => {
                            handledLogin.current =
                              auth.loginCompletion?.generation;
                            loginTarget.current = account.id;
                            auth.addAccount();
                          }}
                        >
                          {t("codexOauth.reauthLogin")}
                        </Button>
                      </>
                    )}
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={locked}
                      aria-label={t("subscriptionProvider.removeAccount", {
                        login: account.login,
                      })}
                      onClick={() => {
                        removedAccount.current = account.id;
                        auth.removeAccount(account.id);
                      }}
                    >
                      {t("subscriptionProvider.removeAccount")}
                    </Button>
                  </div>
                  <SubscriptionAccountQuota
                    provider={source.provider}
                    id={account.id}
                    login={account.login}
                    ready={!account.requires_reauth && !account.reauth_required}
                    disabled={locked}
                  />
                </div>
              ))}
              {!authenticating && (
                <SubscriptionAccountActions
                  disabled={locked}
                  actions={[
                    {
                      key: "add",
                      label: t("subscriptionProvider.addAccount"),
                      onClick: () => {
                        loginTarget.current = undefined;
                        auth.addAccount();
                      },
                    },
                  ]}
                />
              )}
            </div>
            {source.accountId &&
              auth.isStatusSuccess &&
              !auth.accounts.some(
                (account) =>
                  account.id === source.accountId &&
                  !account.requires_reauth &&
                  !account.reauth_required,
              ) && (
                <p role="alert" className="pv-error">
                  {t("codexProvider.reauth")}
                </p>
              )}
            {auth.deviceCode && (
              <div className="cx-login">
                <p>{t("subscriptionProvider.deviceHint")}</p>
                <code>{auth.deviceCode.user_code}</code>
                <Button
                  type="button"
                  variant="ghost"
                  aria-label={t("common.copy")}
                  onClick={() =>
                    void copyText(auth.deviceCode!.user_code).catch((cause) =>
                      onError("copy", cause),
                    )
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
                      .catch((cause) => onError("openWebsite", cause))
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
      )}
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.models")}</span>
        <div className="pv-models">
          {!models.length && (
            <p className="pv-hint">{t("subscriptionProvider.fetchHint")}</p>
          )}
          {!!models.length && (
            <div className="pv-model-actions">
              {models.length > 24 && (
                <Input
                  type="search"
                  className="pv-model-search"
                  value={filter}
                  aria-label={t("providerEditor.filterModels", {
                    count: models.length,
                  })}
                  placeholder={t("providerEditor.filterModels", {
                    count: models.length,
                  })}
                  disabled={locked}
                  onChange={(event) => setFilter(event.target.value)}
                />
              )}
              <button
                type="button"
                disabled={locked}
                onClick={() =>
                  onChange({
                    ...draft,
                    source: {
                      ...source,
                      mode: "picked",
                      models: [
                        ...imagePicks,
                        ...models.filter(
                          (row) =>
                            selected.has(String(row.model ?? row.id)) ||
                            filtered.includes(row),
                        ),
                      ].slice(0, 256),
                    },
                  })
                }
              >
                {t("providerEditor.selectAll")}
              </button>
              <button
                type="button"
                disabled={locked}
                onClick={() =>
                  onChange({
                    ...draft,
                    source: { ...source, mode: "picked", models: imagePicks },
                  })
                }
              >
                {t("providerEditor.selectNone")}
              </button>
            </div>
          )}
          <div
            className="pv-model-chips"
            role="group"
            aria-label={t("modelPicker.models")}
          >
            {filtered.map((row) => {
              const id = String(row.model ?? row.id);
              const checked = selected.has(id);
              return (
                <SubscriptionModelChip
                  key={id}
                  selected={checked}
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
                  {String(
                    (row.modelPreferences as { name?: string })?.name ??
                      row.displayName ??
                      id,
                  )}
                </SubscriptionModelChip>
              );
            })}
          </div>
          <div className="pv-model-footer">
            <div className="pv-add-model">
              <button
                type="button"
                aria-label={t("codexProvider.fetchModels")}
                disabled={locked || !source.accountId}
                onClick={() => void discover()}
              >
                {fetching
                  ? t("common.loading")
                  : t("codexProvider.fetchModels")}
              </button>
            </div>
          </div>
          <p className="pv-hint">{t(`providerParity.hint_${source.mode}`)}</p>
        </div>
      </div>
      <ProviderImageModels
        models={imageModels.map((row) => ({
          id: String(row.model ?? row.id),
          name: String(
            (row.modelPreferences as { name?: string })?.name ??
              row.displayName ??
              row.model ??
              row.id,
          ),
        }))}
        selected={selected}
        disabled={locked}
        onToggle={toggleImage}
      />
    </>
  );
}
