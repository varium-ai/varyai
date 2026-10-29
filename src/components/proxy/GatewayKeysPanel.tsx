import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { providersApi } from "@/lib/api/providers";
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
  takeoverStatus,
}: {
  port: string;
  running?: boolean;
  takeoverStatus?: ProxyTakeoverStatus;
}) {
  const { t } = useTranslation();
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
  const { data: usage = [], isError: usageError } = useQuery({
    queryKey: ["gateway-usage"],
    queryFn: () => gatewayApi.usage(),
    refetchInterval: 5000,
  });
  const { data: desktop } = useQuery({
    queryKey: ["claude-desktop-status"],
    queryFn: providersApi.getClaudeDesktopStatus,
    enabled: view === "agents",
    refetchInterval: 5000,
  });
  const [formOpen, setFormOpen] = useState(false);
  const [name, setName] = useState("");
  const [models, setModels] = useState("");
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
  const [format, setFormat] = useState("anthropic");
  const selectedModels = [
    ...new Set(
      models
        .split(",")
        .map((m) => m.trim())
        .filter(Boolean),
    ),
  ];
  const valid =
    name.trim() &&
    (allModels || selectedModels.length > 0) &&
    (limit === "" ||
      (/^\d+$/.test(limit) && Number.isSafeInteger(Number(limit))));
  const label = (app: string) =>
    APP_ICON_MAP[app as keyof typeof APP_ICON_MAP]?.label ??
    t("gatewayKeys.external");
  async function run(action: () => Promise<unknown>) {
    setPending(true);
    try {
      await action();
      await client.invalidateQueries({ queryKey: ["gateway-keys"] });
      await client.invalidateQueries({ queryKey: ["gateway-usage"] });
    } catch {
      toast.error(t("gatewayKeys.failed"));
    } finally {
      setPending(false);
    }
  }
  function reset() {
    setEditing(null);
    setName("");
    setModels("");
    setAllModels(true);
    setLimit("");
    setSearch("");
    setPeriod("daily");
    setFormOpen(false);
  }
  function edit(key: GatewayKey) {
    setEditing(key.id);
    setName(key.policy.name);
    setModels(key.policy.models.join(", "));
    setAllModels(key.policy.allModels ?? false);
    setLimit(
      key.policy.tokenLimit === null ? "" : String(key.policy.tokenLimit),
    );
    setPeriod(key.policy.period);
    setSecret("");
    setFormOpen(true);
  }
  const connect = keys.find((key) => key.id === connectKey);
  const exampleModel = connect?.policy.models[0] ?? "MODEL_ID";
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
  return (
    <section className="rounded-xl border border-border p-4 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-sm font-semibold">{t("gatewayKeys.title")}</h4>
        <span className="text-xs text-muted-foreground">
          {t(running ? "gatewayKeys.running" : "gatewayKeys.stopped")}
        </span>
        {!running && (
          <Button
            size="sm"
            variant="outline"
            disabled={pending}
            onClick={() =>
              void run(async () => {
                await proxyApi.startProxyServer();
                await client.invalidateQueries({ queryKey: proxyKeys.status });
              })
            }
          >
            {t("gatewayKeys.startRouting")}
          </Button>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        {t("gatewayKeys.description")}
      </p>
      <div
        className="flex flex-wrap gap-2"
        role="tablist"
        aria-label={t("gatewayKeys.title")}
      >
        {(["keys", "agents", "usage", "connect"] as const).map((tab) => (
          <Button
            key={tab}
            role="tab"
            aria-selected={view === tab}
            variant={view === tab ? "default" : "outline"}
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
      {(isError || usageError) && <p role="alert">{t("gatewayKeys.failed")}</p>}
      {view === "keys" && (
        <div className="space-y-3">
          {!formOpen && (
            <Button
              size="sm"
              onClick={() => {
                setSecret("");
                reset();
                setFormOpen(true);
              }}
            >
              {t("gatewayKeys.create")}
            </Button>
          )}
          {formOpen && (
            <form
              className="rounded-lg border p-3 space-y-3"
              onSubmit={(e) => {
                e.preventDefault();
                if (!valid || pending) return;
                const policy: GatewayPolicy = {
                  name: name.trim(),
                  models: allModels ? [] : selectedModels,
                  allModels,
                  tokenLimit: limit === "" ? null : Number(limit),
                  period,
                };
                void run(async () => {
                  if (editing) await gatewayApi.update(editing, policy);
                  else {
                    const created = await gatewayApi.create(policy);
                    setSecret(created.secret);
                  }
                  reset();
                });
              }}
            >
              <h5 className="font-medium text-sm">
                {t(editing ? "gatewayKeys.edit" : "gatewayKeys.create")}
              </h5>
              <div>
                <Label htmlFor="gateway-name">{t("gatewayKeys.name")}</Label>
                <Input
                  id="gateway-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={t("gatewayKeys.nameExample")}
                  required
                  maxLength={120}
                />
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
                  {catalogError && (
                    <p role="alert">{t("gatewayKeys.catalogFailed")}</p>
                  )}
                  <Input
                    aria-label={t("gatewayKeys.searchModels")}
                    placeholder={t("gatewayKeys.searchModels")}
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                  />
                  <div className="max-h-48 overflow-y-auto rounded border p-2 space-y-2">
                    {(catalog?.models ?? [])
                      .filter((m) =>
                        `${m.model} ${m.provider} ${label(m.agent)}`
                          .toLowerCase()
                          .includes(search.toLowerCase()),
                      )
                      .map((m, i) => (
                        <label
                          key={`${m.agent}:${m.provider}:${m.model}:${i}`}
                          className="flex items-center gap-2 text-xs"
                        >
                          <input
                            type="checkbox"
                            checked={selectedModels.includes(m.model)}
                            onChange={(e) =>
                              setModels(
                                (e.target.checked
                                  ? [...selectedModels, m.model]
                                  : selectedModels.filter(
                                      (id) => id !== m.model,
                                    )
                                ).join(", "),
                              )
                            }
                          />
                          <span>
                            {m.model}
                            <span className="text-muted-foreground">
                              {" "}
                              · {label(m.agent)} / {m.provider}
                            </span>
                          </span>
                        </label>
                      ))}
                    {!catalog?.models.length && (
                      <p className="text-xs text-muted-foreground">
                        {t("gatewayKeys.noModels")}
                      </p>
                    )}
                  </div>
                </div>
              )}
              <div>
                <Label htmlFor="gateway-models">
                  {t("gatewayKeys.models")}
                </Label>
                <Input
                  id="gateway-models"
                  value={models}
                  onChange={(e) => {
                    setModels(e.target.value);
                    setAllModels(false);
                  }}
                  placeholder={t("gatewayKeys.manualModels")}
                />
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <div>
                  <Label htmlFor="gateway-limit">
                    {t("gatewayKeys.limit")}
                  </Label>
                  <Input
                    id="gateway-limit"
                    inputMode="numeric"
                    value={limit}
                    onChange={(e) => setLimit(e.target.value)}
                    placeholder={t("gatewayKeys.unlimited")}
                  />
                </div>
                <div>
                  <Label htmlFor="gateway-period">
                    {t("gatewayKeys.period")}
                  </Label>
                  <select
                    id="gateway-period"
                    className="w-full h-9 rounded border bg-background px-2"
                    value={period}
                    onChange={(e) =>
                      setPeriod(e.target.value as GatewayPolicy["period"])
                    }
                  >
                    {(["never", "daily", "monthly"] as const).map((p) => (
                      <option key={p} value={p}>
                        {t(`gatewayKeys.${p}`)}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("gatewayKeys.boundaries")}
              </p>
              <div className="flex gap-2">
                <Button type="submit" disabled={!valid || pending}>
                  {t(editing ? "common.save" : "common.add")}
                </Button>
                <Button type="button" variant="outline" onClick={reset}>
                  {t("common.cancel")}
                </Button>
              </div>
            </form>
          )}
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
                    void run(() => navigator.clipboard.writeText(secret))
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
          {!keys.length && !isError && (
            <p className="rounded border border-dashed p-4 text-sm text-muted-foreground">
              {t("gatewayKeys.empty")}
            </p>
          )}
          {keys.map((key) => {
            const exhausted =
              key.policy.tokenLimit !== null &&
              key.usedTokens >= key.policy.tokenLimit;
            return (
              <article key={key.id} className="rounded-lg border p-3 space-y-2">
                <div className="flex flex-wrap justify-between gap-2">
                  <p className="font-medium text-sm">{key.policy.name}</p>
                  <span className="text-xs">
                    {t(
                      key.revoked
                        ? "gatewayKeys.revoked"
                        : exhausted
                          ? "gatewayKeys.exhausted"
                          : "gatewayKeys.active",
                    )}
                  </span>
                </div>
                <p className="text-xs break-all">
                  {key.policy.allModels
                    ? t("gatewayKeys.allModels")
                    : key.policy.models.join(", ")}
                </p>
                <p className="text-sm">
                  {t("gatewayKeys.usage", {
                    used: key.usedTokens.toLocaleString(),
                    limit: key.policy.tokenLimit?.toLocaleString() ?? "∞",
                  })}
                </p>
                {key.policy.tokenLimit !== null && (
                  <>
                    <progress
                      className="w-full h-2"
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
                      ? new Date(key.resetsAt * 1000).toLocaleString()
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
                <div className="flex flex-wrap gap-2">
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
              </article>
            );
          })}
        </div>
      )}
      {view === "agents" && (
        <div className="space-y-3">
          <p className="text-xs text-muted-foreground">
            {t("gatewayKeys.bindingHint")}
          </p>
          {catalogError && <p role="alert">{t("gatewayKeys.catalogFailed")}</p>}
          <div className="grid gap-3 md:grid-cols-2">
            {APP_IDS.map((app) => {
              const supported =
                catalog?.agents.find((a) => a.id === app)?.localRouting ??
                false;
              const bound = keys.find((key) => key.agents?.includes(app));
              const enabled =
                running &&
                (app === "claude-desktop"
                  ? desktop?.mode === "proxy" &&
                    desktop.configured &&
                    desktop.gatewayTokenConfigured &&
                    desktop.actualBaseUrl === desktop.expectedBaseUrl &&
                    !desktop.staleRawModels &&
                    !desktop.missingRouteMappings
                  : takeoverStatus?.[app]);
              return (
                <div key={app} className="rounded-lg border p-3 space-y-2">
                  <Label htmlFor={`gateway-agent-${app}`}>{label(app)}</Label>
                  {supported ? (
                    <select
                      id={`gateway-agent-${app}`}
                      className="w-full h-9 rounded border bg-background px-2 text-sm"
                      disabled={pending}
                      value={bound?.id ?? ""}
                      onChange={(e) =>
                        void run(() =>
                          gatewayApi.bind(app, e.target.value || null),
                        )
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
                    </select>
                  ) : (
                    <p className="text-xs">{t("gatewayKeys.manualAgent")}</p>
                  )}
                  {supported && (
                    <p className="text-xs text-muted-foreground">
                      {t(
                        !bound
                          ? "gatewayKeys.notBound"
                          : !enabled
                            ? "gatewayKeys.routeRequired"
                            : bound.revoked
                              ? "gatewayKeys.boundRevoked"
                              : "gatewayKeys.bindingActive",
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
        <div className="space-y-3">
          <Label htmlFor="gateway-usage-filter">
            {t("gatewayKeys.filterKey")}
          </Label>
          <select
            id="gateway-usage-filter"
            className="w-full h-9 rounded border bg-background px-2"
            value={usageKey}
            onChange={(e) => setUsageKey(e.target.value)}
          >
            <option value="">{t("gatewayKeys.allKeys")}</option>
            {keys.map((key) => (
              <option key={key.id} value={key.id}>
                {key.policy.name}
              </option>
            ))}
          </select>
          <p className="text-xs text-muted-foreground">
            {t("gatewayKeys.usageHint")}
          </p>
          {!usage.filter((u) => !usageKey || u.keyId === usageKey).length && (
            <p className="rounded border border-dashed p-4 text-sm">
              {t("gatewayKeys.noUsage")}
            </p>
          )}
          {usage
            .filter((u) => !usageKey || u.keyId === usageKey)
            .map((u) => (
              <div
                key={`${u.keyId}:${u.window}:${u.agent}:${u.model}:${u.upstreamModel ?? ""}`}
                className="rounded-lg border p-3 space-y-1 text-xs"
              >
                <div className="break-all text-sm">
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
                  <span>{u.model}</span>
                </div>
                <p>
                  {t("gatewayKeys.usagePeriod", {
                    time: u.window
                      ? new Date(u.window * 1000).toLocaleString()
                      : t("gatewayKeys.never"),
                  })}
                </p>
                <p>
                  {t("gatewayKeys.usageRow", {
                    tokens: u.tokens.toLocaleString(),
                    requests: u.requests,
                    unmetered: u.unmetered,
                  })}
                </p>
                <p>
                  {t("gatewayKeys.lastUsed", {
                    time: new Date(u.lastUsed * 1000).toLocaleString(),
                  })}
                </p>
              </div>
            ))}
        </div>
      )}
      {view === "connect" && (
        <div className="space-y-3">
          <p className="text-xs text-muted-foreground">
            {t("gatewayKeys.connectHint")}
          </p>
          <Label htmlFor="gateway-connect-key">
            {t("gatewayKeys.filterKey")}
          </Label>
          <select
            id="gateway-connect-key"
            className="w-full h-9 rounded border bg-background px-2"
            value={connectKey}
            onChange={(e) => setConnectKey(e.target.value)}
          >
            <option value="">{t("gatewayKeys.chooseKey")}</option>
            {keys
              .filter((key) => !key.revoked)
              .map((key) => (
                <option key={key.id} value={key.id}>
                  {key.policy.name}
                </option>
              ))}
          </select>
          <Label htmlFor="gateway-connect-format">
            {t("gatewayKeys.clientProtocol")}
          </Label>
          <select
            id="gateway-connect-format"
            className="w-full h-9 rounded border bg-background px-2"
            value={format}
            onChange={(e) => setFormat(e.target.value)}
          >
            <option value="anthropic">Anthropic Messages</option>
            <option value="openai_chat">OpenAI Chat Completions</option>
            <option value="openai_responses">OpenAI Responses</option>
            <option value="gemini_native">Gemini</option>
          </select>
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
