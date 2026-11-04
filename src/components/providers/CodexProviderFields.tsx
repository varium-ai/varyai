import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  PlusCircle,
  RefreshCw,
  ArrowUp,
  ArrowDown,
  Copy,
  Check,
  GripVertical,
} from "lucide-react";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
  type CodexProviderSettings,
} from "@/lib/api/routingUpstreams";
import { useCodexProviderQuotas } from "@/lib/query/codexProviderQuota";
import { useCodexOauth } from "./forms/hooks/useCodexOauth";
import { ProviderModelPreferences } from "./ProviderModelPreferences";
import { copyText } from "@/lib/clipboard";
import { open } from "@tauri-apps/plugin-dialog";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";

export function CodexProviderFields({
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
  const [showOthers, setShowOthers] = useState(false);
  const [managing, setManaging] = useState(false);
  const [testing, setTesting] = useState(false);
  const [tests, setTests] = useState<
    Record<string, { ok: boolean; elapsedMs: number }>
  >({});
  const working = fetching || importing || testing || auth.isAddingAccount;
  useEffect(() => {
    onBusyChange(working);
  }, [working, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);
  const locked = disabled || working;
  const rows = [...editor.accounts];
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
  const all = new Map(
    [...catalog, ...policy.models].map((row) => [String(row.model), row]),
  );
  const selected = new Set(policy.models.map((row) => String(row.model)));
  const shown =
    policy.mode === "picked" && !showOthers
      ? policy.models
      : policy.mode === "picked"
        ? [...all.values()]
        : catalog;
  const preferences =
    policy.mode === "picked"
      ? policy.models
      : catalog.map((row) => ({
          ...row,
          ...policy.models.find((p) => p.model === row.model),
        }));
  function update(next: Partial<CodexProviderSettings>) {
    onChange({
      ...draft,
      codex: { ...editor, settings: { ...policy, ...next } },
    });
  }
  function choose(id: string) {
    setShowOthers(true);
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
  async function discover() {
    if (!policy.accounts.length) return;
    const account = policy.accounts[0];
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
  const firstAccount = policy.accounts[0];
  const accountReady = rows.some((row) => row.id === firstAccount && row.ready);
  useEffect(() => {
    if (
      !firstAccount ||
      !accountReady ||
      catalog.length ||
      locked ||
      autoFetched.current.has(firstAccount)
    )
      return;
    autoFetched.current.add(firstAccount);
    void discover();
  }, [firstAccount, accountReady, catalog.length, locked]);
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
      ).slice(0, 30)) {
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
        <span className="pv-field-label">Agent</span>
        <div>
          <span className="cx-agent">Codex</span>
        </div>
      </div>
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
                          : !tiers.length
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
                    {quotaStatus && (
                      <span>{t(`codexProvider.${quotaStatus}`)}</span>
                    )}
                  </div>
                </div>
              );
            })}
            <button
              type="button"
              className="cx-add-account"
              disabled={locked}
              onClick={() => {
                setManaging(true);
                auth.addAccount();
              }}
            >
              <PlusCircle size={16} />
              {t("codexProvider.addAccount")}
            </button>
            <button
              type="button"
              className="cx-add-account"
              disabled={locked}
              onClick={() => void importAccount()}
            >
              <PlusCircle size={16} />
              {t("codexProvider.importAccount")}
            </button>
          </div>
          <p className="pv-hint">{t("codexProvider.accountHint")}</p>
          {managing && auth.deviceCode && (
            <div className="cx-login">
              <p>{t("codexProvider.loginHint")}</p>
              <code>{auth.deviceCode.user_code}</code>
              <button
                type="button"
                onClick={() => void copyText(auth.deviceCode!.user_code)}
                aria-label={t("common.copy")}
              >
                <Copy size={13} />
              </button>
              <button
                type="button"
                onClick={() => {
                  auth.cancelAuth();
                  setManaging(false);
                }}
              >
                {t("common.cancel")}
              </button>
            </div>
          )}
          {managing && auth.error && (
            <p role="alert">{t("codexProvider.loginFailed")}</p>
          )}
        </div>
      </div>
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.models")}</span>
        <div className="pv-stack">
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
          <div className="pv-protocols">
            {(
              [
                ["vendor", "modeVendor"],
                ["picked", "modePicked"],
                ["best", "modeBest"],
                ["usage", "modeUsage"],
              ] as const
            ).map(([mode, label]) => (
              <button
                type="button"
                key={mode}
                aria-pressed={policy.mode === mode}
                className={policy.mode === mode ? "on" : ""}
                disabled={locked}
                onClick={() => {
                  setShowOthers(false);
                  update({
                    mode,
                    ...(mode === "picked" && !policy.models.length
                      ? { models: catalog.slice(0, 24) }
                      : {}),
                  });
                }}
              >
                {t(`codexProvider.${label}`)}
              </button>
            ))}
          </div>
          <div className="pv-probe-actions">
            <button
              type="button"
              disabled={locked}
              onClick={() =>
                update({
                  mode: "picked",
                  models: [...all.values()].slice(0, 256),
                })
              }
            >
              {t("providerEditor.selectAll")}
            </button>
            <button
              type="button"
              disabled={locked}
              onClick={() => update({ mode: "usage", models: [] })}
            >
              {t("providerEditor.selectNone")}
            </button>
          </div>
          <div className="pv-model-chips">
            {shown.map((row) => (
              <button
                type="button"
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
                className={`pv-chip ${selected.has(String(row.model)) && policy.mode === "picked" ? "on" : ""}`}
                aria-pressed={
                  selected.has(String(row.model)) && policy.mode === "picked"
                }
                disabled={locked}
                onClick={() => choose(String(row.model))}
              >
                {selected.has(String(row.model)) &&
                  policy.mode === "picked" && <Check size={12} aria-hidden />}
                {String(
                  (row.modelPreferences as { name?: string })?.name ??
                    row.displayName ??
                    row.model,
                )}
              </button>
            ))}
          </div>
          {policy.mode === "picked" &&
            !showOthers &&
            all.size > selected.size && (
              <button
                type="button"
                className="text"
                disabled={locked}
                onClick={() => setShowOthers(true)}
              >
                {t("codexProvider.showOthers", { n: all.size - selected.size })}
              </button>
            )}
          <div className="pv-probe-actions">
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
              {fetching ? t("common.loading") : t("codexProvider.fetchModels")}
            </button>
            <button
              type="button"
              disabled={
                locked ||
                !policy.accounts.length ||
                !shown.length ||
                policy.mode === "usage"
              }
              onClick={() => void testModels()}
            >
              {testing ? t("common.loading") : t("codexProvider.testModels")}
            </button>
          </div>
          <p className="pv-hint">{t("codexProvider.testHint")}</p>
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
              update({ models: models as Array<Record<string, unknown>> })
            }
          />
          <label className="cx-check">
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
      <div className="pv-field">
        <span className="pv-field-label">{t("codexProvider.endpoint")}</span>
        <div className="cx-endpoint">
          Responses <code>https://chatgpt.com/backend-api/codex</code>
        </div>
      </div>
    </>
  );
}
