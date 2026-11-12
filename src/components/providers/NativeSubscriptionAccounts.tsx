import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { Copy, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  nativeSubscriptionApi as api,
  type NativeSubscriptionProvider,
  type SubscriptionLogin,
} from "@/lib/api/nativeSubscription";
import { SubscriptionAccountQuota } from "./SubscriptionAccountQuota";
import { copyText } from "@/lib/clipboard";
import { settingsApi } from "@/lib/api/settings";
import { SubscriptionAccountActions } from "./SubscriptionAccountActions";

export function NativeSubscriptionAccounts({
  provider,
  accountId,
  disabled,
  editorOpen = true,
  startReauth = false,
  onReauthStarted,
  onSelect,
  onReauthenticated,
  onBusyChange,
  onError,
}: {
  provider: NativeSubscriptionProvider;
  accountId: string;
  disabled: boolean;
  editorOpen?: boolean;
  startReauth?: boolean;
  onReauthStarted?: () => void;
  onSelect: (id: string) => void;
  onReauthenticated?: (id: string) => void;
  onBusyChange: (busy: boolean) => void;
  onError: () => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const key = ["native-subscription-accounts", provider];
  const accounts = useQuery({
    queryKey: key,
    queryFn: () => api.accounts(provider),
    refetchInterval: 15_000,
  });
  const [flow, setFlow] = useState<SubscriptionLogin>();
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string>();
  const [project, setProject] = useState("");
  const active = useRef<string>();
  const mounted = useRef(true);
  const editorOpenRef = useRef(editorOpen);
  editorOpenRef.current = editorOpen;
  useEffect(() => {
    if (editorOpen) return;
    const id = active.current;
    active.current = undefined;
    if (id) void api.cancel(id).catch(() => {});
    setFlow(undefined);
  }, [editorOpen]);
  const loginTarget = useRef<string>();
  const onReauthenticatedRef = useRef(onReauthenticated);
  onReauthenticatedRef.current = onReauthenticated;
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const google = provider === "gemini_cli" || provider === "antigravity";
  const busy = working;
  const authenticating = flow?.state === "waiting";
  useEffect(() => {
    onBusyChange(working);
  }, [working, onBusyChange]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (active.current) void api.cancel(active.current).catch(() => {});
      onBusyChange(false);
    };
  }, [onBusyChange]);
  useEffect(() => {
    setProject(accounts.data?.find((a) => a.id === accountId)?.project ?? "");
  }, [accountId, accounts.data]);
  useEffect(() => {
    if (!flow || flow.state !== "waiting") return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.poll(flow.id);
        if (stopped) return;
        if (next.state !== "waiting") {
          active.current = undefined;
          if (next.account) {
            await client.invalidateQueries({
              queryKey: ["native-subscription-accounts", provider],
            });
            await client.resetQueries({
              queryKey: [
                "subscription-account-quota",
                provider,
                next.account.id,
              ],
            });
            await client.invalidateQueries({ queryKey: ["routing-upstreams"] });
            await client.invalidateQueries({
              queryKey: ["agent-model-catalog"],
            });
            if (!stopped) {
              onSelectRef.current(next.account.id);
              if (loginTarget.current === next.account.id)
                onReauthenticatedRef.current?.(next.account.id);
              loginTarget.current = undefined;
            }
          }
          if (!stopped) {
            setFlow(next);
            if (next.error) setError(next.error);
          }
        } else {
          timer = setTimeout(() => void poll(), 1000);
        }
      } catch {
        if (!stopped) {
          void api.cancel(flow.id).catch(() => {});
          active.current = undefined;
          setFlow(undefined);
          setError("nativeSubscription.failed");
        }
      }
    };
    timer = setTimeout(() => void poll(), 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [flow?.id, flow?.state, client, provider]);
  async function run(action: () => Promise<void>, report = true) {
    setWorking(true);
    setError(undefined);
    try {
      await action();
    } catch (cause) {
      if (mounted.current) {
        setError(
          typeof cause === "string" && cause.startsWith("nativeSubscription.")
            ? cause
            : "nativeSubscription.failed",
        );
        if (report) onError();
      }
    } finally {
      if (mounted.current) setWorking(false);
    }
  }
  async function importAccount(file: boolean) {
    const path = file
      ? await open({
          multiple: false,
          filters: [{ name: "JSON", extensions: ["json"] }],
        })
      : undefined;
    if (file && typeof path !== "string") return;
    const account = await api.import(
      provider,
      typeof path === "string" ? path : undefined,
    );
    await client.invalidateQueries({ queryKey: key });
    if (mounted.current) onSelectRef.current(account.id);
  }
  async function startLogin(targetAccountId?: string) {
    loginTarget.current = targetAccountId;
    const next = targetAccountId
      ? await api.start(provider, targetAccountId)
      : await api.start(provider);
    if (!mounted.current || !editorOpenRef.current) {
      await api.cancel(next.id);
      return;
    }
    active.current = next.id;
    setFlow(next);
    if (next.url) await openLogin(next.url);
  }
  async function openLogin(url: string) {
    try {
      await settingsApi.openExternal(url);
      if (mounted.current) setError(undefined);
    } catch {
      if (mounted.current) setError("nativeSubscription.browserOpenFailed");
    }
  }
  const autoReauthed = useRef(false);
  useEffect(() => {
    if (!startReauth) {
      autoReauthed.current = false;
      return;
    }
    if (
      !editorOpen ||
      authenticating ||
      autoReauthed.current ||
      disabled ||
      busy ||
      !accounts.isSuccess
    )
      return;
    autoReauthed.current = true;
    onReauthStarted?.();
    const targetId =
      accountId && accounts.data.some((account) => account.id === accountId)
        ? accountId
        : undefined;
    void run(() => startLogin(targetId), false);
  }, [
    startReauth,
    editorOpen,
    authenticating,
    disabled,
    busy,
    accounts.isSuccess,
    accounts.data,
    accountId,
    onReauthStarted,
  ]);
  return (
    <div className="pv-field">
      <span className="pv-field-label">{t("codexProvider.accounts")}</span>
      <div className="pv-stack">
        {accounts.isPending && (
          <p role="status" className="pv-hint">
            {t("common.loading")}
          </p>
        )}
        {accounts.isError && (
          <p role="alert" className="pv-error">
            {t("subscriptionProvider.statusFailed")}{" "}
            <Button
              type="button"
              variant="ghost"
              onClick={() => void accounts.refetch()}
            >
              {t("common.retry")}
            </Button>
          </p>
        )}
        <div
          className="cx-accounts"
          role="radiogroup"
          aria-label={t("codexProvider.accounts")}
        >
          {accounts.data?.map((account) => (
            <div key={account.id} className="cx-account">
              <div className="cx-account-name">
                <label className="cx-check">
                  <input
                    type="radio"
                    name={`native-${provider}`}
                    disabled={disabled || busy}
                    checked={accountId === account.id}
                    onChange={() => onSelect(account.id)}
                  />
                  <span>{account.login}</span>
                </label>
                {account.plan && (
                  <span className="cx-plan">{account.plan}</span>
                )}
                {accountId === account.id && (
                  <span className="cx-using">{t("provider.inUse")}</span>
                )}
                {account.reauthRequired && (
                  <>
                    <span className="cx-plan cx-reauth">
                      {t("codexOauth.reauthBadge")}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={disabled || busy || authenticating}
                      aria-label={`${t("codexOauth.reauthLogin")} ${account.login}`}
                      onClick={() =>
                        void run(() => startLogin(account.id), false)
                      }
                    >
                      {t("codexOauth.reauthLogin")}
                    </Button>
                  </>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  disabled={disabled || busy}
                  aria-label={t("nativeSubscription.removeAccount", {
                    login: account.login,
                  })}
                  onClick={() =>
                    void run(async () => {
                      await api.remove(provider, account.id);
                      if (accountId === account.id) onSelect("");
                      await client.invalidateQueries({ queryKey: key });
                    })
                  }
                >
                  {t("subscriptionProvider.removeAccount")}
                </Button>
              </div>
              <SubscriptionAccountQuota
                provider={provider}
                id={account.id}
                login={account.login}
                ready={!account.reauthRequired}
                disabled={disabled || busy}
              />
            </div>
          ))}
          {!authenticating && (
            <SubscriptionAccountActions
              disabled={disabled || busy}
              actions={[
                {
                  key: "add",
                  label:
                    provider === "chatgpt_api"
                      ? t("nativeSubscription.continueChatgpt")
                      : t("subscriptionProvider.addAccount"),
                  onClick: () => void run(() => startLogin(), false),
                },
                ...(provider === "claude" ||
                provider === "gemini_cli" ||
                provider === "cursor"
                  ? [
                      {
                        key: "local",
                        label: t("nativeSubscription.importLocal"),
                        onClick: () => void run(() => importAccount(false)),
                      },
                    ]
                  : []),
                ...(provider !== "chatgpt_api"
                  ? [
                      {
                        key: "file",
                        label: t("nativeSubscription.importFile"),
                        onClick: () => void run(() => importAccount(true)),
                      },
                    ]
                  : []),
              ]}
            />
          )}
        </div>
        {accountId &&
          accounts.isSuccess &&
          !accounts.data.some((account) => account.id === accountId) && (
            <p role="alert" className="pv-error">
              {t("codexProvider.reauth")}
            </p>
          )}
        {flow?.state === "waiting" && (
          <div className="cx-login">
            <p role="status">
              {t(
                provider === "claude"
                  ? "nativeSubscription.claudeWaiting"
                  : "nativeSubscription.browserWaiting",
              )}
            </p>
            {flow.url && (
              <>
                <Input
                  readOnly
                  value={flow.url}
                  aria-label={t("nativeSubscription.authorizationUrl")}
                  className="w-full font-mono text-xs"
                  onFocus={(event) => event.target.select()}
                />
                <Button
                  type="button"
                  variant="ghost"
                  aria-label={t("common.copy")}
                  onClick={() =>
                    void copyText(flow.url!).catch(() =>
                      setError("nativeSubscription.copyFailed"),
                    )
                  }
                >
                  <Copy size={14} />
                  {t("common.copy")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  aria-label={t("subscriptionProvider.openLogin")}
                  onClick={() => void openLogin(flow.url!)}
                >
                  <ExternalLink size={14} />
                  {t("subscriptionProvider.openLogin")}
                </Button>
              </>
            )}
            <Button
              type="button"
              variant="ghost"
              onClick={() =>
                void run(async () => {
                  await api.cancel(flow.id);
                  active.current = undefined;
                  setFlow(undefined);
                })
              }
            >
              {t("common.cancel")}
            </Button>
          </div>
        )}
        {error && (
          <p role="alert" className="pv-error">
            {t(error)}
          </p>
        )}
        <p className="pv-hint">{t(`nativeSubscription.hint_${provider}`)}</p>
        {google && accountId && (
          <label className="pv-stack">
            <span className="pv-field-label">
              {t("nativeSubscription.project")}
            </span>
            <Input
              value={project}
              disabled={disabled || busy}
              onChange={(event) => setProject(event.target.value)}
              placeholder={t("nativeSubscription.projectPlaceholder")}
            />
            <Button
              type="button"
              variant="ghost"
              disabled={disabled || busy}
              onClick={() =>
                void run(async () => {
                  await api.project(provider, accountId, project);
                  await client.invalidateQueries({ queryKey: key });
                })
              }
            >
              {t("common.save")}
            </Button>
          </label>
        )}
      </div>
    </div>
  );
}
