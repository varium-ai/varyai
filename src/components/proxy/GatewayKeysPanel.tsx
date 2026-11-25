import { OptionSelect } from "@/components/ui/option-select";
import { useEffect, useRef, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { KeyRound, Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  fmtInt,
  getLocaleFromLanguage,
  getResolvedLang,
} from "@/components/usage/format";
import { GatewayRuntimeStatus } from "@/components/GatewayRuntimeStatus";
import "./gateway-keys.css";
import {
  APP_IDS,
  APP_ICON_MAP,
  getWorkspaceAgentLabel,
} from "@/config/appConfig";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import { proxyKeys } from "@/lib/query/proxy";
import type { ProxyTakeoverStatus } from "@/types/proxy";
import {
  gatewayApi,
  type GatewayKey,
  type GatewayPolicy,
} from "@/lib/api/gateway";

type View = "keys" | "agents" | "usage" | "connect";
export function GatewayKeysPanel({
  port,
  running = false,
  statusUnavailable,
  takeoverStatus,
}: {
  port: string;
  running?: boolean;
  statusUnavailable?: boolean;
  takeoverStatus?: ProxyTakeoverStatus;
}) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const client = useQueryClient();
  const [view, setView] = useState<View>("keys");
  const { data: keys = [], isError } = useQuery({
    queryKey: ["gateway-keys"],
    queryFn: gatewayApi.list,
    refetchInterval: 5000,
  });
  const { data: catalog, isError: catalogError } = useQuery({
    queryKey: ["gateway-catalog"],
    queryFn: gatewayApi.catalog,
    refetchInterval: 10000,
  });
  const supportedApps = new Set(
    catalog?.agents
      .filter((agent) => agent.localRouting)
      .map((agent) => agent.id),
  );
  const boundKeys = new Map(
    keys.flatMap((key) => (key.agents ?? []).map((app) => [app, key] as const)),
  );
  const connections = useQueries({
    queries: APP_IDS.map((app) => {
      const enabled =
        view === "agents" &&
        running &&
        supportedApps.has(app) &&
        boundKeys.has(app);
      return {
        queryKey: ["agent-connections", app],
        queryFn: () => agentConnectionsApi.get(app),
        enabled,
        refetchInterval: enabled ? 5000 : false,
      };
    }),
  });
  const modelNames = new Map(
    catalog?.models.map((row) => [row.model, row.name]),
  );
  const { data: usage = [], isError: usageError } = useQuery({
    queryKey: ["gateway-usage"],
    queryFn: () => gatewayApi.usage(),
    refetchInterval: 5000,
  });
  const [formOpen, setFormOpen] = useState(false);
  const [name, setName] = useState("");
  const [selectedModels, setSelectedModels] = useState(() => new Set<string>());
  const [allModels, setAllModels] = useState(true);
  const [search, setSearch] = useState("");
  const [limit, setLimit] = useState("");
  const [period, setPeriod] = useState<GatewayPolicy["period"]>("daily");
  const [editing, setEditing] = useState<string | null>(null);
  const [secret, setSecret] = useState("");
  const [pending, setPending] = useState(false);
  const [deleting, setDeleting] = useState<GatewayKey | null>(null);
  const [usageKey, setUsageKey] = useState("");
  const [connectKey, setConnectKey] = useState("");
  const [nameError, setNameError] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const usageRef = useRef<HTMLButtonElement>(null);
  const [format, setFormat] = useState("anthropic");
  useEffect(() => {
    if (view === "keys" && formOpen) {
      if (!editing) {
        formRef.current?.scrollIntoView?.({
          behavior: "smooth",
          block: "start",
        });
      }
      nameRef.current?.focus({ preventScroll: true });
    } else if (view === "usage") {
      usageRef.current?.scrollIntoView?.({
        behavior: "smooth",
        block: "start",
      });
      usageRef.current?.focus({ preventScroll: true });
    }
  }, [view, formOpen, editing, usageKey]);
  const valid =
    name.trim() &&
    (allModels || selectedModels.size > 0) &&
    (limit === "" ||
      (/^\d+$/.test(limit) && Number.isSafeInteger(Number(limit))));
  const label = (app: string) => {
    const knownAgent = APP_IDS.find((id) => id === app);
    return knownAgent
      ? getWorkspaceAgentLabel(t, knownAgent)
      : t("gatewayKeys.external");
  };
  async function run(action: () => Promise<unknown>) {
    setPending(true);
    try {
      await action();
      await client.invalidateQueries({ queryKey: ["gateway-keys"] });
      await client.invalidateQueries({ queryKey: ["gateway-usage"] });
    } catch (error) {
      const duplicate = String(error).includes("gatewayKeys.nameExists");
      if (duplicate) setNameError(true);
      toast.error(
        t(duplicate ? "gatewayKeys.nameExists" : "gatewayKeys.failed"),
      );
    } finally {
      setPending(false);
    }
  }
  function setModelAllowed(id: string, allowed: boolean) {
    setSelectedModels((current) => {
      const next = new Set(current);
      if (allowed) next.add(id);
      else next.delete(id);
      return next;
    });
  }
  function reset() {
    setNameError(false);
    setEditing(null);
    setName("");
    setSelectedModels(new Set());
    setAllModels(true);
    setLimit("");
    setSearch("");
    setPeriod("daily");
    setFormOpen(false);
  }
  function edit(key: GatewayKey) {
    setNameError(false);
    setEditing(key.id);
    setName(key.policy.name);
    setSelectedModels(new Set(key.policy.models));
    setAllModels(key.policy.allModels ?? false);
    setLimit(
      key.policy.tokenLimit === null ? "" : String(key.policy.tokenLimit),
    );
    setPeriod(key.policy.period);
    setSecret("");
    setFormOpen(true);
  }
  const connect = keys.find((key) => key.id === connectKey);
  const exampleModel =
    connect?.policy.models.find((id) => !id.startsWith("vary-provider/")) ??
    "MODEL_ID";
  const base = `http://127.0.0.1:${port}/gateway`;
  const endpoint =
    format === "anthropic"
      ? "/v1/messages"
      : format === "gemini_native"
        ? `/v1beta/models/${encodeURIComponent(exampleModel)}:generateContent`
        : format === "openai_responses"
          ? "/v1/responses"
          : "/v1/chat/completions";
  const exampleBody =
    format === "gemini_native"
      ? { contents: [{ parts: [{ text: "Hello" }] }] }
      : format === "openai_responses"
        ? { model: exampleModel, input: "Hello" }
        : {
            model: exampleModel,
            ...(format === "anthropic" ? { max_tokens: 64 } : {}),
            messages: [{ role: "user", content: "Hello" }],
          };
  const keyForm = formOpen ? (
    <form
      ref={formRef}
      className="gateway-keys-surface gateway-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (!valid || pending) return;
        const unchangedName =
          keys
            .find((key) => key.id === editing)
            ?.policy.name.trim()
            .toLowerCase() === name.trim().toLowerCase();
        if (
          !unchangedName &&
          keys.some(
            (key) =>
              key.id !== editing &&
              key.policy.name.trim().toLowerCase() ===
                name.trim().toLowerCase(),
          )
        ) {
          setNameError(true);
          return;
        }
        const policy: GatewayPolicy = {
          name: name.trim(),
          models: allModels ? [] : [...selectedModels],
          allModels,
          tokenLimit: limit === "" ? null : Number(limit),
          period,
        };
        void run(async () => {
          if (editing) await gatewayApi.update(editing, policy);
          else {
            await gatewayApi.create(policy);
            toast.success(t("gatewayKeys.created"));
          }
          reset();
        });
      }}
    >
      <h4 className="gateway-list-title">
        {t(editing ? "gatewayKeys.edit" : "gatewayKeys.create")}
      </h4>
      <div className="gateway-field">
        <Label htmlFor="gateway-name">{t("gatewayKeys.name")}</Label>
        <Input
          ref={nameRef}
          id="gateway-name"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setNameError(false);
          }}
          placeholder={t("gatewayKeys.nameExample")}
          required
          maxLength={120}
          aria-invalid={nameError}
          aria-describedby={nameError ? "gateway-name-error" : undefined}
        />
        {nameError && (
          <p
            id="gateway-name-error"
            role="alert"
            className="text-xs text-destructive"
          >
            {t("gatewayKeys.nameExists")}
          </p>
        )}
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={allModels}
          onChange={(e) => setAllModels(e.target.checked)}
        />
        {t("gatewayKeys.allModels")}
      </label>
      <p className="text-xs text-muted-foreground">
        {t("gatewayKeys.modelsHint")}
      </p>
      {!allModels && (
        <div className="space-y-2">
          {catalogError && <p role="alert">{t("gatewayKeys.catalogFailed")}</p>}
          <Input
            aria-label={t("gatewayKeys.searchModels")}
            placeholder={t("gatewayKeys.searchModels")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <div className="max-h-48 overflow-y-auto rounded border p-2 space-y-2">
            {(catalog?.models ?? [])
              .filter((m) =>
                `${m.name} ${m.model}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )
              .map((m) => (
                <label
                  key={m.model}
                  className="flex min-w-0 items-center gap-2 text-xs"
                >
                  <input
                    type="checkbox"
                    checked={selectedModels.has(m.model)}
                    onChange={(e) =>
                      setModelAllowed(m.model, e.currentTarget.checked)
                    }
                  />
                  <span className="min-w-0 break-words" title={m.model}>
                    {m.name}
                  </span>
                </label>
              ))}
            {!catalog?.models.length && (
              <p className="text-xs text-muted-foreground">
                {t("gatewayKeys.noModels")}
              </p>
            )}
          </div>
          <div className="gateway-field">
            <p id="gateway-selected-models" className="text-sm font-medium">
              {t("gatewayKeys.models")}
            </p>
            {selectedModels.size ? (
              <ul
                aria-labelledby="gateway-selected-models"
                className="flex max-h-40 flex-wrap gap-1.5 overflow-y-auto p-1 text-xs"
              >
                {[...selectedModels].map((id) => {
                  const modelName = modelNames.get(id);
                  const displayName =
                    modelName || t("gatewayKeys.modelOutsideCatalog");
                  return (
                    <li
                      key={id}
                      title={modelName || id}
                      className="inline-flex max-w-full items-center gap-1 rounded bg-muted py-0.5 pl-2 pr-0.5"
                    >
                      <span className="truncate">{displayName}</span>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0 p-0"
                        aria-label={t("gatewayKeys.removeModel", {
                          name: modelName || id,
                        })}
                        onClick={() => setModelAllowed(id, false)}
                      >
                        <X aria-hidden="true" />
                      </Button>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">
                {t("gatewayKeys.noSelectedModels")}
              </p>
            )}
          </div>
        </div>
      )}
      <div className="gateway-field-grid">
        <div className="gateway-field">
          <Label htmlFor="gateway-limit">{t("gatewayKeys.limit")}</Label>
          <Input
            id="gateway-limit"
            inputMode="numeric"
            value={limit}
            onChange={(e) => setLimit(e.target.value)}
            placeholder={t("gatewayKeys.unlimited")}
          />
        </div>
        <div className="gateway-field">
          <Label htmlFor="gateway-period">{t("gatewayKeys.period")}</Label>
          <OptionSelect
            id="gateway-period"
            value={period}
            onValueChange={(value) =>
              setPeriod(value as GatewayPolicy["period"])
            }
          >
            {(["never", "daily", "monthly"] as const).map((p) => (
              <option key={p} value={p}>
                {t(`gatewayKeys.${p}`)}
              </option>
            ))}
          </OptionSelect>
        </div>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("gatewayKeys.boundaries")}
      </p>
      <div className="gateway-form-actions">
        <Button type="button" variant="outline" onClick={reset}>
          {t("common.cancel")}
        </Button>
        <Button type="submit" disabled={!valid || pending}>
          {t(editing ? "common.save" : "common.add")}
        </Button>
      </div>
    </form>
  ) : null;
  return (
    <section className="gateway-keys">
      <header className="gateway-keys-header gateway-keys-surface">
        <div className="gateway-header-copy">
          <div className="gateway-header-title">
            <h2>{t("gatewayKeys.title")}</h2>
            <GatewayRuntimeStatus
              running={running}
              unavailable={statusUnavailable}
            />
          </div>
          <p className="gateway-supporting-text">
            {t("gatewayKeys.description")}
          </p>
        </div>
        {!running && (
          <Button
            size="sm"
            variant="outline"
            disabled={pending}
            onClick={() =>
              void run(async () => {
                await proxyApi.startProxyServer();
                await client.invalidateQueries({
                  queryKey: proxyKeys.status,
                });
              })
            }
          >
            {t("gatewayKeys.startRouting")}
          </Button>
        )}
      </header>
      <div className="gateway-keys-toolbar">
        <div
          className="gateway-tabs vary-segmented"
          role="tablist"
          aria-label={t("gatewayKeys.title")}
        >
          {(["keys", "agents", "usage", "connect"] as const).map((tab) => (
            <Button
              key={tab}
              role="tab"
              aria-selected={view === tab}
              variant="ghost"
              size="sm"
              disabled={pending}
              onClick={() => {
                setSecret("");
                setView(tab);
              }}
            >
              {t(`gatewayKeys.tabs.${tab}`)}
            </Button>
          ))}
        </div>
        {view === "keys" && !formOpen && (
          <Button
            size="sm"
            onClick={() => {
              setSecret("");
              reset();
              setFormOpen(true);
            }}
          >
            <Plus size={16} aria-hidden="true" />
            {t("gatewayKeys.create")}
          </Button>
        )}
      </div>
      {(isError || usageError) && (
        <p role="alert" className="text-destructive">
          {t("gatewayKeys.failed")}
        </p>
      )}
      {view === "keys" && (
        <div className="gateway-key-directory">
          {!editing && keyForm}
          <Dialog
            open={Boolean(secret)}
            onOpenChange={(open) => {
              if (!open) setSecret("");
            }}
          >
            <DialogContent className="p-6 gap-3">
              <DialogTitle>{t("gatewayKeys.reveal")}</DialogTitle>
              <DialogDescription>
                {t("gatewayKeys.savedSecret")}
              </DialogDescription>
              <code className="block break-all select-all text-xs">
                {secret}
              </code>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    void run(async () => {
                      await navigator.clipboard.writeText(secret);
                      toast.success(t("gatewayKeys.copied"));
                    })
                  }
                >
                  {t("common.copy")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setSecret("")}
                >
                  {t("gatewayKeys.dismiss")}
                </Button>
              </div>
            </DialogContent>
          </Dialog>
          {!keys.length && !isError && !formOpen && (
            <div className="gateway-keys-surface gateway-empty">
              <KeyRound size={20} strokeWidth={1.5} aria-hidden="true" />
              <p>{t("gatewayKeys.empty")}</p>
            </div>
          )}
          <div className="gateway-keys-surface gateway-key-list">
            {keys.map((key) => {
              const exhausted =
                key.policy.tokenLimit !== null &&
                key.usedTokens >= key.policy.tokenLimit;
              return (
                <article key={key.id} className="gateway-key-entry">
                  <div className="gateway-key-heading">
                    <p
                      className="gateway-list-title min-w-0 flex-1 truncate"
                      title={key.policy.name}
                    >
                      {key.policy.name}
                    </p>
                    <span
                      className="gateway-key-status"
                      data-tone={
                        key.revoked
                          ? "neutral"
                          : exhausted
                            ? "warning"
                            : "success"
                      }
                    >
                      {t(
                        key.revoked
                          ? "gatewayKeys.revoked"
                          : exhausted
                            ? "gatewayKeys.exhausted"
                            : "gatewayKeys.active",
                      )}
                    </span>
                  </div>
                  {key.policy.allModels ? (
                    <p className="text-xs">{t("gatewayKeys.allModels")}</p>
                  ) : (
                    <ul className="flex flex-wrap gap-1 text-xs">
                      {key.policy.models.map((id) => (
                        <li
                          key={id}
                          title={id}
                          className="max-w-full truncate rounded bg-muted px-2 py-0.5"
                        >
                          {modelNames.get(id) || id}
                        </li>
                      ))}
                    </ul>
                  )}
                  <p className="text-sm">
                    {t("gatewayKeys.usage", {
                      used: fmtInt(key.usedTokens, locale),
                      limit: fmtInt(key.policy.tokenLimit, locale, "∞"),
                    })}
                  </p>
                  {key.policy.tokenLimit !== null && (
                    <>
                      <progress
                        className="gateway-budget-progress"
                        aria-label={t("gatewayKeys.budgetProgress")}
                        max={Math.max(1, key.policy.tokenLimit)}
                        value={Math.min(key.usedTokens, key.policy.tokenLimit)}
                      />
                      <p className="text-xs">
                        {t("gatewayKeys.remaining", {
                          count: Math.max(
                            0,
                            key.policy.tokenLimit - key.usedTokens,
                          ),
                        })}
                      </p>
                    </>
                  )}
                  <p className="text-xs text-muted-foreground">
                    {t("gatewayKeys.reset", {
                      time: key.resetsAt
                        ? new Date(key.resetsAt * 1000).toLocaleString(locale)
                        : t("gatewayKeys.never"),
                    })}
                  </p>
                  <p className="text-xs">
                    {t("gatewayKeys.boundAgents", {
                      agents: key.agents?.length
                        ? key.agents.map(label).join(", ")
                        : t("gatewayKeys.noneBound"),
                    })}
                  </p>
                  <div className="gateway-key-actions">
                    {!key.revoked && (
                      <>
                        {key.secretAvailable && (
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={pending}
                            onClick={() =>
                              void run(async () => {
                                const value = await gatewayApi.reveal(key.id);
                                setSecret(value ?? "");
                                if (!value)
                                  toast.error(t("gatewayKeys.legacySecret"));
                              })
                            }
                          >
                            {t("gatewayKeys.reveal")}
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={pending}
                          onClick={() => edit(key)}
                        >
                          {t("gatewayKeys.edit")}
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={pending}
                          onClick={() =>
                            void run(async () => {
                              setSecret("");
                              await gatewayApi.revoke(key.id);
                            })
                          }
                        >
                          {t("gatewayKeys.revoke")}
                        </Button>
                      </>
                    )}
                    {key.revoked && (
                      <Button
                        size="sm"
                        variant="destructive"
                        disabled={pending || !!key.agents?.length}
                        onClick={() => setDeleting(key)}
                      >
                        {t("common.delete")}
                      </Button>
                    )}
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setSecret("");
                        setUsageKey(key.id);
                        setView("usage");
                      }}
                    >
                      {t("gatewayKeys.viewUsage")}
                    </Button>
                  </div>
                  {!key.revoked && !key.secretAvailable && (
                    <p className="text-xs text-muted-foreground">
                      {t("gatewayKeys.legacySecret")}
                    </p>
                  )}
                  {key.revoked && !!key.agents?.length && (
                    <p className="text-xs text-muted-foreground">
                      {t("gatewayKeys.deleteUnbindHint")}
                    </p>
                  )}
                  {editing === key.id && keyForm}
                </article>
              );
            })}
          </div>
        </div>
      )}
      {view === "agents" && (
        <div className="gateway-key-directory">
          <p className="gateway-supporting-text">
            {t("gatewayKeys.bindingHint")}
          </p>
          {catalogError && <p role="alert">{t("gatewayKeys.catalogFailed")}</p>}
          <div className="gateway-agent-grid">
            {APP_IDS.map((app, index) => {
              const supported = supportedApps.has(app);
              const bound = boundKeys.get(app);
              const status = connections[index];
              const connection = status.data;
              const enabled =
                running &&
                (takeoverStatus?.[app] ||
                  (connection?.enabled &&
                    connection.configured &&
                    (!connection.recoveryPending ||
                      connection.recoveryReason ===
                        "bridge_selection_pending")));
              return (
                <div key={app} className="gateway-keys-surface gateway-agent">
                  <div className="flex items-center gap-2">
                    <span aria-hidden="true">{APP_ICON_MAP[app].icon}</span>
                    <Label
                      className="gateway-list-title"
                      htmlFor={`gateway-agent-${app}`}
                    >
                      {label(app)}
                    </Label>
                  </div>
                  {supported ? (
                    <OptionSelect
                      id={`gateway-agent-${app}`}
                      disabled={pending}
                      value={bound?.id ?? ""}
                      onValueChange={(value) =>
                        void run(() => gatewayApi.bind(app, value || null))
                      }
                    >
                      <option value="">{t("gatewayKeys.noBinding")}</option>
                      {keys
                        .filter((key) => !key.revoked || key.id === bound?.id)
                        .map((key) => (
                          <option
                            key={key.id}
                            value={key.id}
                            disabled={key.revoked}
                          >
                            {key.policy.name}
                            {key.revoked
                              ? ` (${t("gatewayKeys.revoked")})`
                              : ""}
                          </option>
                        ))}
                    </OptionSelect>
                  ) : (
                    <p className="text-xs">{t("gatewayKeys.manualAgent")}</p>
                  )}
                  {supported && (
                    <p className="text-xs text-muted-foreground">
                      {t(
                        !bound
                          ? "gatewayKeys.notBound"
                          : enabled
                            ? bound.revoked
                              ? "gatewayKeys.boundRevoked"
                              : "gatewayKeys.bindingActive"
                            : running && !connection
                              ? status.isError
                                ? "gatewayKeys.routeStatusUnavailable"
                                : "common.loading"
                              : "gatewayKeys.routeRequired",
                      )}
                    </p>
                  )}
                  {app === "qoder" && (
                    <p className="text-xs text-muted-foreground">
                      {t("gatewayKeys.qoderHint")}
                    </p>
                  )}
                  {app === "trae" && (
                    <p className="text-xs text-muted-foreground">
                      {t("gatewayKeys.traeHint")}
                    </p>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
      {view === "usage" && (
        <div className="gateway-key-directory">
          <div className="gateway-keys-surface gateway-usage-filter">
            <Label htmlFor="gateway-usage-filter">
              {t("gatewayKeys.filterKey")}
            </Label>
            <OptionSelect
              ref={usageRef}
              id="gateway-usage-filter"
              className="min-w-0 scroll-mt-4"
              value={usageKey}
              onValueChange={(value) => setUsageKey(value)}
            >
              <option value="">{t("gatewayKeys.allKeys")}</option>
              {keys.map((key) => (
                <option key={key.id} value={key.id}>
                  {key.policy.name}
                </option>
              ))}
            </OptionSelect>
            <p className="text-xs text-muted-foreground">
              {t("gatewayKeys.usageHint")}
            </p>
          </div>
          {!usage.filter((u) => !usageKey || u.keyId === usageKey).length && (
            <p className="gateway-keys-surface gateway-empty">
              {t("gatewayKeys.noUsage")}
            </p>
          )}
          <div className="gateway-keys-surface gateway-key-list">
            {usage
              .filter((u) => !usageKey || u.keyId === usageKey)
              .map((u) => (
                <div
                  key={`${u.keyId}:${u.window}:${u.agent}:${u.model}:${u.upstreamModel ?? ""}`}
                  className="gateway-usage-entry"
                >
                  <div className="gateway-list-title break-words">
                    <span>{t("gatewayKeys.upstreamModel")}</span>:{" "}
                    <span className="font-semibold">
                      {u.upstreamModel ?? t("gatewayKeys.upstreamMissing")}
                    </span>
                  </div>
                  <p className="text-muted-foreground">
                    {keys.find((key) => key.id === u.keyId)?.policy.name ??
                      u.keyId}{" "}
                    · {t("gatewayKeys.sourceAgent")}: {label(u.agent)}
                  </p>
                  <div className="break-all text-muted-foreground">
                    <span>{t("gatewayKeys.requestModel")}</span>:{" "}
                    <span title={u.model}>
                      {modelNames.get(u.model) || u.model}
                    </span>
                  </div>
                  <p>
                    {t("gatewayKeys.usagePeriod", {
                      time: u.window
                        ? new Date(u.window * 1000).toLocaleString(locale)
                        : t("gatewayKeys.never"),
                    })}
                  </p>
                  <p>
                    {t("gatewayKeys.usageRow", {
                      tokens: fmtInt(u.tokens, locale),
                      requests: fmtInt(u.requests, locale),
                      unmetered: fmtInt(u.unmetered, locale),
                    })}
                  </p>
                  <p>
                    {t("gatewayKeys.lastUsed", {
                      time: new Date(u.lastUsed * 1000).toLocaleString(locale),
                    })}
                  </p>
                </div>
              ))}
          </div>
        </div>
      )}
      {view === "connect" && (
        <div className="gateway-keys-surface gateway-connect">
          <p className="gateway-supporting-text">
            {t("gatewayKeys.connectHint")}
          </p>
          <Label htmlFor="gateway-connect-key">
            {t("gatewayKeys.filterKey")}
          </Label>
          <OptionSelect
            id="gateway-connect-key"
            value={connectKey}
            onValueChange={(value) => setConnectKey(value)}
          >
            <option value="">{t("gatewayKeys.chooseKey")}</option>
            {keys
              .filter((key) => !key.revoked)
              .map((key) => (
                <option key={key.id} value={key.id}>
                  {key.policy.name}
                </option>
              ))}
          </OptionSelect>
          <Label htmlFor="gateway-connect-format">
            {t("gatewayKeys.clientProtocol")}
          </Label>
          <Select value={format} onValueChange={setFormat}>
            <SelectTrigger
              id="gateway-connect-format"
              className="vary-task-select"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="vary-task-select-menu" sideOffset={8}>
              <SelectItem value="anthropic">Anthropic Messages</SelectItem>
              <SelectItem value="openai_chat">
                OpenAI Chat Completions
              </SelectItem>
              <SelectItem value="openai_responses">OpenAI Responses</SelectItem>
              <SelectItem value="gemini_native">Gemini</SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs">{t("gatewayKeys.exampleHint")}</p>
          <pre className="overflow-x-auto rounded bg-muted p-3 text-xs">{`curl '${base}${endpoint}' \\\n  -H 'Authorization: Bearer YOUR_GATEWAY_KEY' \\\n  -H 'Content-Type: application/json' \\\n${format === "anthropic" ? "  -H 'anthropic-version: 2023-06-01' \\\n" : ""}  --data '${JSON.stringify(exampleBody)}'`}</pre>
          <p className="text-xs text-muted-foreground">
            {t("gatewayKeys.secretHint")}
          </p>
        </div>
      )}
      <ConfirmDialog
        isOpen={deleting !== null}
        title={t("gatewayKeys.deleteTitle")}
        message={t("gatewayKeys.deleteConfirm", {
          name: deleting?.policy.name,
        })}
        confirmText={t("common.delete")}
        pending={pending}
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          if (!deleting || pending) return;
          const id = deleting.id;
          void run(async () => {
            await gatewayApi.delete(id);
            setDeleting(null);
            setSecret("");
            if (usageKey === id) setUsageKey("");
            if (connectKey === id) setConnectKey("");
          });
        }}
      />
    </section>
  );
}
