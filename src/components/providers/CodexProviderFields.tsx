import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw, ArrowUp, ArrowDown, GripVertical } from "lucide-react";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
  type CodexProviderSettings,
} from "@/lib/api/routingUpstreams";
import { useCodexProviderQuotas } from "@/lib/query/codexProviderQuota";
import { useCodexOauth } from "./forms/hooks/useCodexOauth";
import { CodexLoginPanel } from "./CodexLoginPanel";
import { isImageGenerationModel } from "@/lib/providerModelKind";
import { ProviderImageModels } from "./ProviderImageModels";
import { ProviderModelPreferences } from "./ProviderModelPreferences";
import { settingsApi } from "@/lib/api/settings";
import { copyText } from "@/lib/clipboard";
import { open } from "@tauri-apps/plugin-dialog";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import { SubscriptionAccountActions } from "./SubscriptionAccountActions";
import { SubscriptionModelChip } from "./SubscriptionModelChip";

export function CodexProviderFields({
  draft,
  disabled,
  editorOpen = true,
  startReauth = false,
  onReauthStarted,
  onChange,
  onError,
  onBusyChange,
}: {
  draft: RoutingUpstream;
  disabled: boolean;
  editorOpen?: boolean;
  startReauth?: boolean;
  onReauthStarted?: () => void;
  onChange: (value: RoutingUpstream) => void;
  onError: () => void;
  onBusyChange: (value: boolean) => void;
}) {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const editor = draft.codex!;
  const policy = editor.settings;
  const auth = useCodexOauth();
  const [catalogs, setCatalogs] = useState<
    Record<string, typeof editor.models>
  >({
    [policy.accounts[0] ?? "native"]: editor.models,
  });
  const catalog = catalogs[policy.accounts[0]] ?? [];
  const [fetching, setFetching] = useState(false);
  const [catalogStatus, setCatalogStatus] = useState<"empty" | "failed" | null>(
    null,
  );
  const autoFetched = useRef(new Set<string>());
  const [importing, setImporting] = useState(false);
  const [typed, setTyped] = useState("");
  const [filter, setFilter] = useState("");
  const [managing, setManaging] = useState(false);
  const [loginWay, setLoginWay] = useState<"browser" | "device">("browser");
  const [loginProxy, setLoginProxy] = useState<string>();
  const [loginActionError, setLoginActionError] = useState<string>();
  const loginTarget = useRef<string>();
  const loginActionGeneration = useRef(0);
  const editorOpenRef = useRef(editorOpen);
  editorOpenRef.current = editorOpen;
  const browser = auth.browser;
  const completion =
    loginWay === "browser" && browser
      ? browser.completion
      : auth.loginCompletion;
  async function discover(accountOverride?: string) {
    const account = accountOverride ?? policy.accounts[0];
    if (!account) return;
    setFetching(true);
    setCatalogStatus(null);
    try {
      const models = await routingUpstreamsApi.codexModels(
        draft.id,
        draft.revision,
        account,
      );
      setCatalogs((old) => ({ ...old, [account]: models }));
      setCatalogStatus(models.length ? null : "empty");
      await Promise.all(
        [
          "intent-routing-groups",
          "intent-routing-providers",
          "agent-model-catalog",
          "routing-catalog-discovery",
        ].map((key) => client.invalidateQueries({ queryKey: [key] })),
      );
    } catch {
      setCatalogStatus("failed");
      onError();
    } finally {
      setFetching(false);
    }
  }
  useEffect(
    () => () => {
      loginActionGeneration.current += 1;
    },
    [],
  );
  useEffect(() => {
    if (!editorOpen) {
      loginActionGeneration.current += 1;
      void auth.cancelAuth();
      void browser?.cancel();
      setManaging(false);
    }
  }, [editorOpen, auth.cancelAuth, browser?.cancel]);
  const reauthSource = useRef<string>();
  const handledLogin = useRef<number>();
  useEffect(() => {
    if (
      !managing ||
      !completion ||
      handledLogin.current === completion.generation
    )
      return;
    handledLogin.current = completion.generation;
    const accountId = completion.accountId;
    const source = reauthSource.current;
    reauthSource.current = undefined;
    // Keep unsaved model choices when switching an explicitly reauthenticated credential source.
    if (source && source !== accountId) {
      onChange({
        ...draft,
        codex: {
          ...editor,
          settings: {
            ...policy,
            accounts: [
              ...new Set(
                policy.accounts.map((id) => (id === source ? accountId : id)),
              ),
            ],
          },
          accounts: editor.accounts
            .filter((row) => row.id !== source)
            .map((row) => ({
              ...row,
              aliases: row.aliases?.filter((id) => id !== source),
            })),
        },
      });
    }
    if (!source && !policy.accounts.includes(accountId)) {
      onChange({
        ...draft,
        codex: {
          ...editor,
          settings: { ...policy, accounts: [...policy.accounts, accountId] },
        },
      });
    }
    setManaging(false);
    autoFetched.current.add(accountId);
    setCatalogs((old) => {
      const next = { ...old };
      delete next[accountId];
      return next;
    });
    void discover(accountId);
    void client.resetQueries({ queryKey: ["codex-provider-quota", accountId] });
    for (const key of [
      "routing-upstreams",
      "intent-routing-providers",
      "agent-model-catalog",
      "routing-catalog-discovery",
    ]) {
      void client.invalidateQueries({ queryKey: [key] });
    }
  }, [completion, managing, client, draft, editor, policy, onChange]);
  const [testing, setTesting] = useState(false);
  const [tests, setTests] = useState<
    Record<string, { ok: boolean; elapsedMs: number }>
  >({});
  const working = fetching || importing || testing;
  const authenticating = auth.isAddingAccount || !!browser?.isPending;
  useEffect(() => {
    onBusyChange(working);
  }, [working, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);
  const locked = disabled || working;
  const rows = editor.accounts.map((row) => {
    const current =
      !row.native && auth.accounts?.find((account) => account.id === row.id);
    return current
      ? { ...row, login: current.login, ready: !current.reauth_required }
      : row;
  });
  const aliases = new Set(rows.flatMap((row) => row.aliases ?? []));
  for (const account of auth.accounts ?? []) {
    if (!aliases.has(account.id) && !rows.some((row) => row.id === account.id))
      rows.push({
        id: account.id,
        login: account.login,
        native: false,
        ready: !account.reauth_required,
      });
  }
  for (const id of policy.accounts) {
    if (!aliases.has(id) && !rows.some((row) => row.id === id))
      rows.push({
        id,
        login: t("codexProvider.missingAccount"),
        native: false,
        ready: false,
      });
  }
  const ids = [
    ...policy.accounts,
    ...rows.map((row) => row.id).filter((id) => !policy.accounts.includes(id)),
  ];
  const ordered = ids.flatMap((id) => rows.find((row) => row.id === id) ?? []);
  const quotas = useCodexProviderQuotas(ordered);
  function reauthIdFor(account: (typeof ordered)[number]) {
    return account.native
      ? account.aliases?.find(
          (id) =>
            id !== "native" &&
            (auth.accounts?.some((row) => row.id === id) ||
              (!auth.isStatusSuccess &&
                editor.accounts.some(
                  (row) =>
                    (!row.native && row.id === id) || row.aliases?.includes(id),
                ))),
        )
      : auth.accounts?.some((row) => row.id === account.id) ||
          (!auth.isStatusSuccess &&
            editor.accounts.some((row) => !row.native && row.id === account.id))
        ? account.id
        : undefined;
  }
  async function beginLogin(
    way: "browser" | "device",
    selection?: { proxy?: string },
  ) {
    const proxy = selection ? selection.proxy : loginProxy;
    const attempt = ++loginActionGeneration.current;
    handledLogin.current =
      way === "browser" && browser
        ? browser.completion?.generation
        : auth.loginCompletion?.generation;
    setLoginWay(way);
    setLoginActionError(undefined);
    setManaging(true);
    if (way === "browser" && browser) {
      await auth.cancelAuth();
      if (editorOpenRef.current && attempt === loginActionGeneration.current)
        await browser.start(loginTarget.current, proxy);
    } else {
      if (!browser) {
        if (loginTarget.current) auth.reauthAccount(loginTarget.current);
        else auth.addAccount();
        return;
      }
      await browser.cancel();
      if (!editorOpenRef.current || attempt !== loginActionGeneration.current)
        return;
      if (loginTarget.current)
        auth.reauthAccount(loginTarget.current, ...(browser ? [proxy] : []));
      else if (browser && proxy !== undefined) auth.addAccountWithProxy(proxy);
      else auth.addAccount();
    }
  }
  function startAccountReauth(account?: (typeof ordered)[number]) {
    reauthSource.current = account?.id;
    loginTarget.current = account ? reauthIdFor(account) : undefined;
    setLoginProxy(undefined);
    void beginLogin("browser", { proxy: undefined });
  }
  async function cancelLogin() {
    loginActionGeneration.current += 1;
    setManaging(false);
    await Promise.all([auth.cancelAuth(), browser?.cancel()]);
  }
  const autoReauthed = useRef(false);
  useEffect(() => {
    if (!startReauth) {
      autoReauthed.current = false;
      return;
    }
    if (!editorOpen || autoReauthed.current || locked || authenticating) return;
    autoReauthed.current = true;
    onReauthStarted?.();
    const target =
      ordered.find(
        (account) => policy.accounts.includes(account.id) && !account.ready,
      ) ?? ordered.find((account) => !account.ready);
    startAccountReauth(target);
  }, [
    startReauth,
    editorOpen,
    locked,
    authenticating,
    ordered,
    policy.accounts,
    onReauthStarted,
  ]);
  const all = new Map(
    [...catalog, ...policy.models].map((row) => [String(row.model), row]),
  );
  const selected = new Set(policy.models.map((row) => String(row.model)));
  const chatModels = [...all.values()].filter(
    (row) => !isImageGenerationModel(row),
  );
  const imageModels = [...all.values()].filter(isImageGenerationModel);
  const imagePicks = policy.models.filter(isImageGenerationModel);
  const term = filter.trim().toLocaleLowerCase();
  const matches = (row: Record<string, unknown>) =>
    `${row.model} ${(row.modelPreferences as { name?: string })?.name ?? row.displayName ?? ""}`
      .toLocaleLowerCase()
      .includes(term);
  const shown = chatModels.filter(matches);
  const preferences = (
    policy.mode === "picked"
      ? policy.models
      : catalog.map((row) => ({
          ...row,
          ...policy.models.find((p) => p.model === row.model),
        }))
  ).filter((row) => !isImageGenerationModel(row));
  function update(next: Partial<CodexProviderSettings>) {
    onChange({
      ...draft,
      codex: { ...editor, settings: { ...policy, ...next } },
    });
  }
  function choose(id: string) {
    update({
      mode: "picked",
      models:
        selected.has(id) && policy.mode === "picked"
          ? policy.models.filter((row) => row.model !== id)
          : selected.has(id)
            ? policy.models
            : [...policy.models, all.get(id) ?? { model: id }],
    });
  }
  const firstAccount = policy.accounts[0];
  const accountReady = rows.some((row) => row.id === firstAccount && row.ready);
  useEffect(() => {
    if (
      !editorOpen ||
      !firstAccount ||
      !accountReady ||
      catalog.length ||
      locked ||
      autoFetched.current.has(firstAccount)
    )
      return;
    autoFetched.current.add(firstAccount);
    void discover();
  }, [editorOpen, firstAccount, accountReady, catalog.length, locked]);
  async function importAccount() {
    setImporting(true);
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Codex account", extensions: ["json"] }],
      });
      if (typeof path !== "string") return;
      await routingUpstreamsApi.importCodexAccount(path);
      await auth.refetchStatus();
    } catch {
      onError();
    } finally {
      setImporting(false);
    }
  }
  async function testModels() {
    const account = policy.accounts[0];
    if (!account) return;
    setTesting(true);
    setTests({});
    try {
      for (const row of (policy.mode === "picked"
        ? policy.models
        : catalog.slice(0, 24)
      )
        .filter((row) => !isImageGenerationModel(row))
        .slice(0, 30)) {
        const model = String(row.model);
        const result = await routingUpstreamsApi.testCodexModel(
          draft.id,
          draft.revision,
          account,
          model,
        );
        setTests((old) => ({ ...old, [model]: result }));
      }
    } catch {
      onError();
    } finally {
      setTesting(false);
    }
  }
  function move(id: string, offset: number) {
    if (locked) return;
    const next = [...policy.accounts];
    const at = next.indexOf(id);
    const to = at + offset;
    if (at < 0 || to < 0 || to >= next.length) return;
    [next[at], next[to]] = [next[to], next[at]];
    update({ accounts: next });
  }
  const drag = useRoutingRowDrag("data-codex-account", (from, to) => {
    const next = [...policy.accounts];
    const source = next.indexOf(from);
    const destination = next.indexOf(to);
    if (locked || source < 0 || destination < 0) return;
    next.splice(destination, 0, ...next.splice(source, 1));
    update({ accounts: next });
  });
  return (
    <>
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.accounts")}</span>
        <div>
          <div className="cx-accounts" data-routing-list>
            {ordered.map((account, index) => {
              const query = quotas[index];
              const quota = query.data?.quota;
              const tiers =
                account.ready &&
                quota?.credentialStatus === "valid" &&
                quota.success
                  ? quota.tiers.filter((tier) =>
                      Number.isFinite(tier.utilization),
                    )
                  : [];
              const needsLogin =
                !account.ready ||
                quota?.credentialStatus === "expired" ||
                quota?.credentialStatus === "parse_error" ||
                quota?.credentialStatus === "not_found";
              const quotaHeld =
                quota?.codex?.allowed === false ||
                quota?.codex?.limitReached === true ||
                quota?.codex?.spendControl?.reached === true;
              const creditBalance = quota?.codex?.credits?.balance;
              const quotaStatus =
                quota?.credentialStatus === "expired"
                  ? "quotaLoginRequired"
                  : quota?.credentialStatus === "parse_error"
                    ? "quotaCredentialsInvalid"
                    : !account.ready || quota?.credentialStatus === "not_found"
                      ? "quotaCredentialsMissing"
                      : !query.data
                        ? "quotaLoading"
                        : query.data.failed
                          ? tiers.length
                            ? "quotaStale"
                            : "quotaFailed"
                          : !tiers.length && !quotaHeld && creditBalance == null
                            ? "quotaEmpty"
                            : null;
              return (
                <div
                  key={account.id}
                  className="cx-account"
                  role="group"
                  aria-label={account.login}
                  data-codex-account={
                    policy.accounts.includes(account.id)
                      ? account.id
                      : undefined
                  }
                  tabIndex={0}
                  onKeyDown={(event) => {
                    if (
                      event.altKey &&
                      ["ArrowUp", "ArrowDown"].includes(event.key)
                    ) {
                      event.preventDefault();
                      move(account.id, event.key === "ArrowUp" ? -1 : 1);
                    }
                  }}
                >
                  <div className="cx-account-name">
                    {policy.accounts.length > 1 &&
                      policy.accounts.includes(account.id) && (
                        <button
                          type="button"
                          aria-label={`${t("codexProvider.reorder")} ${account.login}`}
                          disabled={locked}
                          onPointerDown={(event) => {
                            if (!locked) drag.start(event, account.id);
                          }}
                          onClick={(event) => {
                            if (drag.consumeClick(account.id))
                              event.preventDefault();
                          }}
                        >
                          <GripVertical size={12} />
                        </button>
                      )}
                    <label>
                      <input
                        type="checkbox"
                        checked={policy.accounts.includes(account.id)}
                        disabled={
                          locked ||
                          (!account.ready &&
                            !policy.accounts.includes(account.id))
                        }
                        onChange={(event) =>
                          update({
                            accounts: event.target.checked
                              ? [...policy.accounts, account.id]
                              : policy.accounts.filter(
                                  (id) => id !== account.id,
                                ),
                          })
                        }
                      />
                      <b>{account.login}</b>
                    </label>
                    {account.plan && (
                      <span className="cx-plan">{account.plan}</span>
                    )}
                    {needsLogin && (
                      <span className="cx-plan cx-reauth">
                        {t("codexOauth.reauthBadge")}
                      </span>
                    )}
                    {account.native && (
                      <span className="cx-plan">
                        {t("codexProvider.native")}
                      </span>
                    )}
                    <button
                      type="button"
                      title={t("codexProvider.refreshQuota")}
                      aria-label={`${t("codexProvider.refreshQuota")} ${account.login}`}
                      disabled={locked || !account.ready || query.isFetching}
                      onClick={() =>
                        void query.refetch({ cancelRefetch: false })
                      }
                    >
                      <RefreshCw
                        size={12}
                        className={
                          query.isFetching
                            ? "animate-spin motion-reduce:animate-none"
                            : undefined
                        }
                      />
                    </button>
                    {needsLogin && (
                      <button
                        type="button"
                        aria-label={`${t("codexOauth.reauthLogin")} ${account.login}`}
                        disabled={locked || authenticating}
                        onClick={() => startAccountReauth(account)}
                      >
                        {t("codexOauth.reauthLogin")}
                      </button>
                    )}
                    {policy.accounts.length > 1 &&
                      policy.accounts.includes(account.id) && (
                        <>
                          <button
                            type="button"
                            aria-label={`${t("codexProvider.moveUp")} ${account.login}`}
                            disabled={
                              locked || policy.accounts[0] === account.id
                            }
                            onClick={() => move(account.id, -1)}
                          >
                            <ArrowUp size={12} />
                          </button>
                          <button
                            type="button"
                            aria-label={`${t("codexProvider.moveDown")} ${account.login}`}
                            disabled={
                              locked || policy.accounts.at(-1) === account.id
                            }
                            onClick={() => move(account.id, 1)}
                          >
                            <ArrowDown size={12} />
                          </button>
                        </>
                      )}
                  </div>
                  {!account.ready && (
                    <p className="pv-hint">{t("codexProvider.reauth")}</p>
                  )}
                  <div
                    className="cx-quota"
                    role="status"
                    aria-busy={query.isFetching}
                  >
                    {tiers.map((tier) => (
                      <div key={tier.name}>
                        <span>
                          {t(
                            `subscription.${tier.name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())}`,
                            {
                              defaultValue: tier.name,
                            },
                          )}
                        </span>
                        <span className="cx-meter">
                          <i
                            style={{
                              width: `${Math.max(0, Math.min(100, 100 - tier.utilization))}%`,
                            }}
                          />
                        </span>
                        <b>
                          {t("codexProvider.remaining", {
                            n: Math.max(
                              0,
                              Math.min(100, Math.round(100 - tier.utilization)),
                            ),
                          })}
                        </b>
                        {tier.resetsAt && (
                          <span>
                            {t("codexProvider.resets", {
                              time: new Date(tier.resetsAt).toLocaleString(
                                i18n.language,
                              ),
                            })}
                          </span>
                        )}
                      </div>
                    ))}
                    {quotaHeld && <span>{t("codexProvider.quotaHeld")}</span>}
                    {creditBalance != null && (
                      <span>
                        {t("codexProvider.creditBalance", {
                          balance: creditBalance,
                        })}
                      </span>
                    )}
                    {quotaStatus && (
                      <span>{t(`codexProvider.${quotaStatus}`)}</span>
                    )}
                  </div>
                </div>
              );
            })}
            {!authenticating && !managing && (
              <SubscriptionAccountActions
                disabled={locked}
                actions={[
                  {
                    key: "add",
                    label: t("codexProvider.addAccount"),
                    onClick: () => startAccountReauth(),
                  },
                  {
                    key: "file",
                    label: t("codexProvider.importAccount"),
                    onClick: () => void importAccount(),
                  },
                ]}
              />
            )}
            {managing && (
              <CodexLoginPanel
                key={`${loginWay}-${browser?.flow?.id ?? auth.deviceCode?.device_code ?? "starting"}`}
                mode={loginWay}
                proxy={loginProxy}
                inheritProxy={!!loginTarget.current}
                onInheritProxy={() => {
                  setLoginProxy(undefined);
                  void beginLogin(loginWay, { proxy: undefined });
                }}
                url={
                  loginWay === "browser" && browser
                    ? browser.flow?.url
                    : auth.deviceCode?.verification_uri
                }
                code={
                  loginWay === "device" || !browser
                    ? auth.deviceCode?.user_code
                    : undefined
                }
                starting={
                  !!browser?.starting ||
                  (!browser?.flow &&
                    !auth.deviceCode &&
                    !browser?.error &&
                    !auth.error)
                }
                submitting={browser?.submitting}
                submitted={browser?.submitted}
                callbackBlocked={browser?.flow?.callbackBlocked}
                httpStatus={browser?.flow?.diagnostic?.httpStatus}
                openFailed={browser?.openFailed}
                error={
                  loginWay === "browser" && browser
                    ? (browser.error ?? loginActionError)
                    : auth.error
                      ? "codexProvider.loginFailed"
                      : loginActionError
                }
                onCancel={() => void cancelLogin()}
                onRetry={
                  (
                    loginWay === "browser" && browser
                      ? browser.error || loginActionError
                      : auth.error || loginActionError
                  )
                    ? () => void beginLogin(loginWay)
                    : undefined
                }
                onReopen={() => {
                  if (loginWay === "browser" && browser) void browser.reopen();
                  else if (auth.deviceCode)
                    void settingsApi
                      .openExternal(auth.deviceCode.verification_uri)
                      .catch(() => {});
                }}
                onCopy={() => {
                  const value =
                    loginWay === "browser" && browser
                      ? browser.flow?.url
                      : auth.deviceCode?.verification_uri;
                  if (value)
                    void copyText(value).catch(() =>
                      setLoginActionError("nativeSubscription.copyFailed"),
                    );
                }}
                onDevice={() =>
                  void beginLogin(loginWay === "browser" ? "device" : "browser")
                }
                onImport={() => void cancelLogin().then(importAccount)}
                onProxy={(proxy) => {
                  setLoginProxy(proxy);
                  void beginLogin(loginWay, { proxy });
                }}
                onSubmit={(url) => {
                  if (browser) void browser.submit(url);
                }}
              />
            )}
          </div>
          <p className="pv-hint">{t("codexProvider.accountHint")}</p>
        </div>
      </div>
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.models")}</span>
        <div className="pv-models">
          {!catalog.length && (
            <p className="pv-hint" role="status">
              {t(
                fetching
                  ? "codexProvider.catalogLoading"
                  : !accountReady
                    ? "codexProvider.catalogLoginRequired"
                    : catalogStatus === "failed"
                      ? "codexProvider.catalogFailed"
                      : "codexProvider.catalogEmpty",
              )}
            </p>
          )}
          <div className="pv-model-actions">
            {chatModels.length > 24 && (
              <input
                type="search"
                className="pv-model-search"
                value={filter}
                aria-label={t("providerEditor.filterModels", {
                  count: chatModels.length,
                })}
                placeholder={t("providerEditor.filterModels", {
                  count: chatModels.length,
                })}
                disabled={locked}
                onChange={(event) => setFilter(event.target.value)}
              />
            )}
            <button
              type="button"
              disabled={locked}
              onClick={() =>
                update({
                  mode: "picked",
                  models: [
                    ...imagePicks,
                    ...chatModels.filter(
                      (row) => selected.has(String(row.model)) || matches(row),
                    ),
                  ].slice(0, 256),
                })
              }
            >
              {t("providerEditor.selectAll")}
            </button>
            <button
              type="button"
              disabled={locked}
              onClick={() => update({ mode: "picked", models: imagePicks })}
            >
              {t("providerEditor.selectNone")}
            </button>
          </div>
          <div
            className="pv-model-chips"
            role="group"
            aria-label={t("modelPicker.models")}
          >
            {shown.map((row) => (
              <SubscriptionModelChip
                key={String(row.model)}
                title={
                  tests[String(row.model)]
                    ? t(
                        tests[String(row.model)].ok
                          ? "codexProvider.testPassed"
                          : "codexProvider.testFailed",
                        { ms: tests[String(row.model)].elapsedMs },
                      )
                    : undefined
                }
                selected={
                  selected.has(String(row.model)) && policy.mode === "picked"
                }
                disabled={locked}
                onClick={() => choose(String(row.model))}
              >
                {String(
                  (row.modelPreferences as { name?: string })?.name ??
                    row.displayName ??
                    row.model,
                )}
              </SubscriptionModelChip>
            ))}
          </div>
          <div className="pv-model-footer">
            <div className="pv-add-model">
              <input
                aria-label={t("providerEditor.addModelId")}
                placeholder={t("providerEditor.addModelId")}
                value={typed}
                disabled={locked}
                onChange={(event) => setTyped(event.target.value)}
              />
              <button
                type="button"
                disabled={locked || !typed.trim() || selected.size >= 256}
                onClick={() => {
                  const id = typed.trim();
                  if (!selected.has(id))
                    update({
                      mode: "picked",
                      models: [...policy.models, { model: id }],
                    });
                  setTyped("");
                }}
              >
                {t("providerEditor.addModel")}
              </button>
              <button
                type="button"
                disabled={locked || !policy.accounts.length}
                onClick={() => void discover()}
              >
                {fetching
                  ? t("common.loading")
                  : t("codexProvider.fetchModels")}
              </button>
              <button
                type="button"
                disabled={
                  locked ||
                  !policy.accounts.length ||
                  !shown.length ||
                  policy.mode === "usage"
                }
                title={t("codexProvider.testHint")}
                onClick={() => void testModels()}
              >
                {testing ? t("common.loading") : t("codexProvider.testModels")}
              </button>
            </div>
            {!!Object.keys(tests).length && (
              <div className="cx-tests" role="status">
                {Object.entries(tests).map(([model, result]) => (
                  <p key={model}>
                    <code>{model}</code>{" "}
                    {t(
                      result.ok
                        ? "codexProvider.testPassed"
                        : "codexProvider.testFailed",
                      { ms: result.elapsedMs },
                    )}
                  </p>
                ))}
              </div>
            )}
            <ProviderModelPreferences
              models={preferences}
              disabled={locked}
              onChange={(models) =>
                update({
                  models: [...models, ...imagePicks] as Array<
                    Record<string, unknown>
                  >,
                })
              }
            />
          </div>
          <label className="cx-check pv-groups-only">
            <input
              type="checkbox"
              checked={policy.groupsOnly}
              disabled={locked}
              onChange={(event) => update({ groupsOnly: event.target.checked })}
            />
            {t("codexProvider.groupsOnly")}
          </label>
          <p className="pv-hint">{t(`codexProvider.${policy.mode}Hint`)}</p>
        </div>
      </div>
      <ProviderImageModels
        models={imageModels.map((row) => ({
          id: String(row.model),
          name: String(
            (row.modelPreferences as { name?: string })?.name ??
              row.displayName ??
              row.model,
          ),
        }))}
        selected={selected}
        disabled={locked}
        onToggle={(id) =>
          update({
            models: selected.has(id)
              ? policy.models.filter((row) => row.model !== id)
              : [...policy.models, all.get(id)!],
          })
        }
      />
    </>
  );
}
