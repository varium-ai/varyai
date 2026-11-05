import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { ExternalLink, Plus, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  nativeSubscriptionApi as api,
  type NativeSubscriptionProvider,
  type SubscriptionLogin,
} from "@/lib/api/nativeSubscription";
import { settingsApi } from "@/lib/api/settings";

export function NativeSubscriptionAccounts({
  provider,
  accountId,
  disabled,
  onSelect,
  onBusyChange,
  onError,
}: {
  provider: NativeSubscriptionProvider;
  accountId: string;
  disabled: boolean;
  onSelect: (id: string) => void;
  onBusyChange: (busy: boolean) => void;
  onError: () => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const key = ["native-subscription-accounts", provider];
  const accounts = useQuery({
    queryKey: key,
    queryFn: () => api.accounts(provider),
  });
  const [flow, setFlow] = useState<SubscriptionLogin>();
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string>();
  const [project, setProject] = useState("");
  const active = useRef<string>();
  const mounted = useRef(true);
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const google = provider === "gemini_cli" || provider === "antigravity";
  const busy = working || flow?.state === "waiting";
  useEffect(() => {
    onBusyChange(busy);
  }, [busy, onBusyChange]);
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
            if (!stopped) onSelectRef.current(next.account.id);
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
  async function run(action: () => Promise<void>) {
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
        onError();
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
            <label key={account.id} className="cx-account cx-check">
              <input
                type="radio"
                name={`native-${provider}`}
                disabled={disabled || busy}
                checked={accountId === account.id}
                onChange={() => onSelect(account.id)}
              />
              <span>{account.login}</span>
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
                {t("common.delete")}
              </Button>
            </label>
          ))}
        </div>
        {accountId &&
          accounts.isSuccess &&
          !accounts.data.some((account) => account.id === accountId) && (
            <p role="alert" className="pv-error">
              {t("codexProvider.reauth")}
            </p>
          )}
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            disabled={disabled || busy}
            onClick={() =>
              void run(async () => {
                const next = await api.start(provider);
                if (!mounted.current) {
                  await api.cancel(next.id);
                  return;
                }
                active.current = next.id;
                setFlow(next);
                if (next.url) await settingsApi.openExternal(next.url);
              })
            }
          >
            <Plus size={14} />
            {provider === "chatgpt_api"
              ? t("nativeSubscription.continueChatgpt")
              : t("subscriptionProvider.addAccount")}
          </Button>
          {(provider === "claude" || provider === "gemini_cli") && (
            <Button
              type="button"
              variant="ghost"
              disabled={disabled || busy}
              onClick={() => void run(() => importAccount(false))}
            >
              <Upload size={14} />
              {t("nativeSubscription.importLocal")}
            </Button>
          )}
          {provider !== "chatgpt_api" && (
            <Button
              type="button"
              variant="ghost"
              disabled={disabled || busy}
              onClick={() => void run(() => importAccount(true))}
            >
              {t("nativeSubscription.importFile")}
            </Button>
          )}
        </div>
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
              <Button
                type="button"
                variant="ghost"
                aria-label={t("subscriptionProvider.openLogin")}
                onClick={() =>
                  void run(() => settingsApi.openExternal(flow.url!))
                }
              >
                <ExternalLink size={14} />
              </Button>
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
