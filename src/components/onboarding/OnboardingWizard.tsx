import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import {
  ArrowDownToLine,
  ArrowLeft,
  Check,
  Copy,
  Link,
  Loader2,
  UserRound,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ProviderImportDialog } from "@/components/providers/ProviderImportDialog";
import { SubscriptionSignIn } from "@/components/providers/SubscriptionSignIn";
import {
  subscriptionProviders,
  type SubscriptionProviderId,
} from "@/config/subscriptionProviders";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { copyText } from "@/lib/clipboard";
import { DRAG_REGION_ATTR, isMac } from "@/lib/platform";
import appIcon from "@/assets/icons/app-icon.png";
import "./onboarding.css";

type Method = "key" | "subscription" | "import";
type ApiFormat = Extract<
  RoutingUpstream["source"],
  { kind: "api" }
>["apiFormat"];

export function OnboardingWizard({
  onFinish,
}: {
  onFinish: (connected: boolean) => Promise<void>;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [method, setMethod] = useState<Method>();
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [format, setFormat] = useState<ApiFormat>("openai_chat");
  const [signing, setSigning] = useState<SubscriptionProviderId>();
  const [account, setAccount] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");
  const [copied, setCopied] = useState(false);
  const saving = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const locked = busy || importBusy;
  useEffect(() => {
    heading.current?.focus();
  }, [method]);
  useEffect(() => {
    if (!locked) return;
    const prevent = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", prevent);
    return () => window.removeEventListener("beforeunload", prevent);
  }, [locked]);

  async function refresh() {
    await Promise.all(
      ["routing-upstreams", "agent-model-catalog", "intent-routing-groups"].map(
        (key) => client.invalidateQueries({ queryKey: [key] }),
      ),
    );
  }
  async function finish(connected: boolean) {
    if (saving.current || importBusy) return;
    saving.current = true;
    setBusy(true);
    setError("");
    try {
      await onFinish(connected);
    } catch {
      setError("onboarding.finishFailed");
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  async function connect(event: React.FormEvent) {
    event.preventDefault();
    if (saving.current) return;
    setError("");
    let url: URL;
    try {
      url = new URL(baseUrl.trim());
      if (
        !["http:", "https:"].includes(url.protocol) ||
        url.username ||
        url.password ||
        url.search ||
        url.hash ||
        !apiKey.trim()
      )
        throw new Error("invalid");
    } catch {
      setError("onboarding.invalidConnection");
      return;
    }
    saving.current = true;
    setBusy(true);
    try {
      const key = apiKey.trim();
      const header =
        format === "anthropic"
          ? "x-api-key"
          : format === "gemini_native"
            ? "x-goog-api-key"
            : "Authorization";
      const found = await fetchModelsForConfig(
        baseUrl.trim(),
        "",
        false,
        undefined,
        undefined,
        {
          apiFormat: format,
          requestHeaders: {
            [header]: header === "Authorization" ? `Bearer ${key}` : key,
          },
        },
      );
      if (!found.length) {
        setError("onboarding.noModels");
        return;
      }
      await routingUpstreamsApi.save({
        schemaVersion: 1,
        id: "",
        revision: 0,
        name: url.hostname,
        hasApiKey: false,
        source: {
          kind: "api",
          baseUrl: baseUrl.trim(),
          apiFormat: format,
          apiKey: key,
          ...(format === "anthropic" ? { authType: "api-key" as const } : {}),
          models: [],
          catalog: found.map(({ id, context_window, ...metadata }) => ({
            model: id,
            ...metadata,
            ...(context_window
              ? { contextWindow: context_window, contextSource: "vendor" }
              : {}),
          })),
          mode: "vendor",
          groupsOnly: false,
        },
      });
      setApiKey("");
      setSuccess(t("onboarding.connected", { count: found.length }));
      void refresh();
    } catch {
      setError("onboarding.connectionFailed");
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  async function saveAccount(
    provider: SubscriptionProviderId,
    accountId: string,
  ) {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setError("");
    setAccount(accountId);
    try {
      const records = await routingUpstreamsApi.list();
      const existing = records.find((row) =>
        provider === "codex"
          ? row.source.kind === "codex"
          : row.source.kind === "subscription" &&
            row.source.provider === provider,
      );
      if (provider === "codex") {
        const draft = existing
          ? await routingUpstreamsApi.editor(existing.id, existing.revision)
          : await routingUpstreamsApi.createCodex();
        if (!draft.codex) throw new Error("unavailable");
        await routingUpstreamsApi.saveCodex(draft, {
          ...draft.codex.settings,
          accounts: [
            ...new Set([
              ...(existing ? draft.codex.settings.accounts : []),
              accountId,
            ]),
          ],
        });
      } else {
        const preset = subscriptionProviders.find((row) => row.id === provider);
        if (!preset) throw new Error("unavailable");
        await routingUpstreamsApi.save(
          existing?.source.kind === "subscription"
            ? {
                ...existing,
                source:
                  existing.source.accountId === accountId
                    ? existing.source
                    : {
                        ...existing.source,
                        accountId,
                        models: [],
                        catalog: [],
                        mode: "vendor",
                      },
              }
            : {
                schemaVersion: 1,
                id: "",
                revision: 0,
                name: preset.name,
                icon: preset.icon,
                hasApiKey: false,
                source: {
                  kind: "subscription",
                  provider,
                  accountId,
                  models: [],
                  catalog: [],
                  mode: "vendor",
                  groupsOnly: false,
                },
              },
        );
      }
      setSigning(undefined);
      setSuccess(t("onboarding.accountConnected"));
      void refresh();
    } catch {
      setError("onboarding.accountFailed");
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  const methods = [
    {
      id: "key",
      icon: Link,
      title: "keyTitle",
      description: "keyDescription",
      action: "keyAction",
    },
    {
      id: "subscription",
      icon: UserRound,
      title: "subscriptionTitle",
      description: "subscriptionDescription",
      action: "subscriptionAction",
    },
    {
      id: "import",
      icon: ArrowDownToLine,
      title: "importTitle",
      description: "importDescription",
      action: "importAction",
    },
  ] as const;
  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent
        variant="fullscreen"
        className="vary-onboarding"
        aria-describedby="onboarding-description"
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          heading.current?.focus();
        }}
        onEscapeKeyDown={(event) => event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        {isMac() && <div {...DRAG_REGION_ATTR} className="ob-drag-region" />}
        <header className="ob-header">
          <span className="ob-brand">
            <img src={appIcon} alt="" width={26} height={26} />
            Vary AI
          </span>
          <ol className="ob-steps" aria-label={t("onboarding.progress")}>
            <li aria-current={!method ? "step" : undefined} className="active">
              <span>{method ? <Check size={14} aria-hidden /> : 1}</span>
              {t("onboarding.chooseStep")}
            </li>
            <li
              aria-current={method ? "step" : undefined}
              className={method ? "active" : ""}
            >
              <span>2</span>
              {t("onboarding.connectStep")}
            </li>
          </ol>
          <Button
            variant="ghost"
            disabled={locked}
            onClick={() => void finish(!!success)}
          >
            {t("onboarding.skip")}
          </Button>
        </header>
        <div className="ob-scroll">
          <div className="ob-content">
            <DialogTitle asChild>
              <h1 ref={heading} tabIndex={-1} className="ob-title">
                {t(method ? "onboarding.stepTwo" : "onboarding.stepOne")}
              </h1>
            </DialogTitle>
            <DialogDescription
              id="onboarding-description"
              className="ob-description"
            >
              {t(
                method
                  ? `onboarding.${method}Description`
                  : "onboarding.description",
              )}
            </DialogDescription>
            {!method && (
              <>
                <div className="ob-methods">
                  {methods.map((item) => (
                    <article key={item.id} className="ob-method">
                      <span className="ob-method-icon">
                        <item.icon size={25} strokeWidth={1.5} aria-hidden />
                      </span>
                      <h2>{t(`onboarding.${item.title}`)}</h2>
                      <p>{t(`onboarding.${item.description}`)}</p>
                      {item.id === "subscription" && (
                        <ul className="ob-method-list">
                          {subscriptionProviders.map((p) => (
                            <li key={p.id}>{p.name}</li>
                          ))}
                        </ul>
                      )}
                      {item.id === "import" && (
                        <ul className="ob-method-list">
                          {[
                            "CC Switch",
                            "Claude Code",
                            "Codex",
                            "OpenCode",
                            "Magpie",
                          ].map((name) => (
                            <li key={name}>{name}</li>
                          ))}
                        </ul>
                      )}
                      <Button
                        variant="outline"
                        onClick={() => {
                          setMethod(item.id);
                          setError("");
                        }}
                      >
                        {t(`onboarding.${item.action}`)}
                      </Button>
                    </article>
                  ))}
                </div>
                <div className="ob-help">
                  <p>{t("onboarding.help")}</p>
                  <Button
                    variant="outline"
                    onClick={() => {
                      void copyText(t("onboarding.helpPrompt"))
                        .then(() => setCopied(true))
                        .catch(() => setError("onboarding.copyFailed"));
                    }}
                  >
                    {copied ? <Check size={15} /> : <Copy size={15} />}
                    {t(copied ? "common.copied" : "common.copy")}
                  </Button>
                </div>
              </>
            )}
            {method === "key" && !success && (
              <form
                className="ob-key-form"
                onSubmit={(event) => void connect(event)}
              >
                <label htmlFor="onboarding-url">{t("onboarding.url")}</label>
                <Input
                  id="onboarding-url"
                  type="url"
                  placeholder="https://api.example.com/v1"
                  autoComplete="off"
                  value={baseUrl}
                  disabled={locked}
                  required
                  onChange={(event) => {
                    setBaseUrl(event.target.value);
                    setError("");
                  }}
                />
                <label htmlFor="onboarding-key">{t("onboarding.key")}</label>
                <Input
                  id="onboarding-key"
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  value={apiKey}
                  disabled={locked}
                  required
                  onChange={(event) => {
                    setApiKey(event.target.value);
                    setError("");
                  }}
                />
                <p className="ob-hint">{t("onboarding.keyHint")}</p>
                <label htmlFor="onboarding-protocol">
                  {t("onboarding.protocol")}
                </label>
                <select
                  id="onboarding-protocol"
                  value={format}
                  disabled={locked}
                  onChange={(event) =>
                    setFormat(event.target.value as ApiFormat)
                  }
                >
                  <option value="openai_chat">OpenAI Chat Completions</option>
                  <option value="openai_responses">OpenAI Responses</option>
                  <option value="anthropic">Anthropic Messages</option>
                  <option value="gemini_native">Gemini API</option>
                </select>
                <Button
                  type="submit"
                  className="ob-connect"
                  disabled={locked || !baseUrl.trim() || !apiKey.trim()}
                >
                  {busy && <Loader2 size={15} className="animate-spin" />}
                  {t(busy ? "onboarding.connecting" : "onboarding.connect")}
                </Button>
              </form>
            )}
            {method === "subscription" && !success && (
              <>
                <div className="ob-subscriptions">
                  {subscriptionProviders.map((provider) => (
                    <button
                      key={provider.id}
                      className={signing === provider.id ? "selected" : ""}
                      aria-label={provider.name}
                      aria-pressed={signing === provider.id}
                      disabled={locked}
                      onClick={() => {
                        setSigning(provider.id);
                        setAccount(undefined);
                        setError("");
                      }}
                    >
                      <span>
                        <ProviderIcon
                          icon={provider.icon}
                          name={provider.name}
                          size={25}
                        />
                      </span>
                      {provider.name}
                    </button>
                  ))}
                </div>
                {signing && !account && (
                  <SubscriptionSignIn
                    key={signing}
                    provider={signing}
                    onCancel={() => setSigning(undefined)}
                    onComplete={(id) => void saveAccount(signing, id)}
                  />
                )}
                {account && signing && !busy && (
                  <Button onClick={() => void saveAccount(signing, account)}>
                    {t("common.retry")}
                  </Button>
                )}
                {busy && <p role="status">{t("onboarding.savingAccount")}</p>}
                {signing === "codex" && (
                  <p className="ob-hint">{t("onboarding.codexHint")}</p>
                )}
              </>
            )}
            {method === "import" && !success && (
              <ProviderImportDialog
                open
                embedded
                onBusyChange={setImportBusy}
                onClose={() => {
                  setMethod(undefined);
                  setError("");
                }}
                onImported={async () => {
                  setSuccess(t("onboarding.imported"));
                  void refresh();
                }}
              />
            )}
            {success && (
              <div className="ob-success" role="status">
                <Check size={19} aria-hidden />
                <div>
                  <strong>{success}</strong>
                  <p>{t("onboarding.nextHint")}</p>
                </div>
              </div>
            )}
            {error && (
              <p className="ob-error" role="alert">
                {t(error)}
              </p>
            )}
          </div>
        </div>
        {method && (
          <footer className="ob-footer">
            {method && !success ? (
              <Button
                variant="outline"
                disabled={locked}
                onClick={() => {
                  setMethod(undefined);
                  setSigning(undefined);
                  setAccount(undefined);
                  setError("");
                }}
              >
                <ArrowLeft size={15} />
                {t("common.back")}
              </Button>
            ) : (
              <span />
            )}
            {success && (
              <Button disabled={locked} onClick={() => void finish(true)}>
                {busy && <Loader2 size={15} className="animate-spin" />}
                {t("onboarding.enterTools")}
              </Button>
            )}
          </footer>
        )}
      </DialogContent>
    </Dialog>
  );
}
