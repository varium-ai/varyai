import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import {
  subscriptionProviders,
  type SubscriptionProviderId,
} from "@/config/subscriptionProviders";
import {
  nativeSubscriptionApi,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { settingsApi } from "@/lib/api/settings";
import { copyText } from "@/lib/clipboard";
import { CodexLoginPanel } from "./CodexLoginPanel";
import { useCodexOauth } from "./forms/hooks/useCodexOauth";
import { useManagedAuth } from "./forms/hooks/useManagedAuth";
import { useNativeSubscriptionLogin } from "./forms/hooks/useNativeSubscriptionLogin";

type Props = {
  provider: SubscriptionProviderId;
  onCancel: () => void;
  onComplete: (id: string) => void;
};
export function SubscriptionSignIn(props: Props) {
  const client = useQueryClient();
  const mounted = useRef(true);
  const completed = useRef(props.onComplete);
  completed.current = props.onComplete;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const onComplete = async (id: string) => {
    await Promise.all([
      client.resetQueries({
        queryKey:
          props.provider === "codex"
            ? ["codex-provider-quota", id]
            : ["subscription-account-quota", props.provider, id],
      }),
      client.invalidateQueries({ queryKey: ["routing-upstreams"] }),
      client.invalidateQueries({ queryKey: ["agent-model-catalog"] }),
    ]);
    if (mounted.current) completed.current(id);
  };
  if (props.provider === "codex")
    return <CodexSignIn {...props} onComplete={(id) => void onComplete(id)} />;
  if (props.provider === "github_copilot" || props.provider === "xai_oauth")
    return (
      <DeviceSignIn
        {...props}
        provider={props.provider}
        onComplete={(id) => void onComplete(id)}
      />
    );
  return (
    <NativeSignIn
      {...props}
      provider={props.provider}
      onComplete={(id) => void onComplete(id)}
    />
  );
}
function useCompletion(
  id: string | undefined,
  onComplete: Props["onComplete"],
) {
  const callback = useRef(onComplete);
  callback.current = onComplete;
  const handled = useRef<string>();
  useEffect(() => {
    if (id && handled.current !== id) {
      handled.current = id;
      callback.current(id);
    }
  }, [id]);
}
function CodexSignIn({ onCancel, onComplete }: Props) {
  const auth = useCodexOauth();
  const browser = auth.browser;
  const client = useQueryClient();
  const [mode, setMode] = useState<"browser" | "device">("browser");
  const [proxy, setProxy] = useState("");
  const [error, setError] = useState<string>();
  const generation = useRef(0);
  const completion =
    mode === "browser" && browser
      ? browser.completion?.accountId
      : auth.loginCompletion?.accountId;
  useCompletion(completion, onComplete);
  async function start(way: "browser" | "device", selectedProxy = proxy) {
    const attempt = ++generation.current;
    setMode(way);
    setError(undefined);
    if (way === "browser" && browser) {
      await auth.cancelAuth();
      if (attempt === generation.current)
        await browser.start(undefined, selectedProxy);
    } else {
      await browser?.cancel();
      if (attempt === generation.current)
        auth.addAccountWithProxy(selectedProxy);
    }
  }
  useEffect(() => {
    void start("browser");
    return () => {
      generation.current += 1;
      void auth.cancelAuth();
      void browser?.cancel();
    };
  }, []);
  const url =
    mode === "browser" && browser
      ? browser.flow?.url
      : auth.deviceCode?.verification_uri;
  const failure =
    error ??
    (mode === "browser" && browser
      ? browser.error
      : auth.error
        ? "codexBrowser.failed"
        : undefined);
  return (
    <CodexLoginPanel
      mode={mode}
      proxy={proxy}
      url={url}
      code={mode === "device" ? auth.deviceCode?.user_code : undefined}
      starting={
        mode === "browser" && browser
          ? browser.starting
          : !auth.deviceCode && !auth.error
      }
      submitting={browser?.submitting}
      submitted={browser?.submitted}
      error={failure}
      callbackBlocked={browser?.flow?.callbackBlocked}
      openFailed={browser?.openFailed}
      onCancel={() => {
        generation.current += 1;
        void auth.cancelAuth();
        void browser?.cancel();
        onCancel();
      }}
      onReopen={() => {
        if (mode === "browser" && browser) void browser.reopen();
        else if (url)
          void settingsApi
            .openExternal(url)
            .catch(() => setError("codexBrowser.openFailed"));
      }}
      onCopy={() => {
        if (url)
          void copyText(url).catch(() =>
            setError("nativeSubscription.copyFailed"),
          );
      }}
      onCodeCopy={() => {
        if (auth.deviceCode)
          void copyText(auth.deviceCode.user_code).catch(() =>
            setError("nativeSubscription.copyFailed"),
          );
      }}
      onDevice={() => void start(mode === "browser" ? "device" : "browser")}
      onProxy={(value) => {
        setProxy(value);
        void start(mode, value);
      }}
      onSubmit={
        mode === "browser" && browser
          ? (value) => void browser.submit(value)
          : undefined
      }
      onRetry={failure ? () => void start(mode) : undefined}
      onImport={() => {
        void (async () => {
          const attempt = ++generation.current;
          await Promise.all([auth.cancelAuth(), browser?.cancel()]);
          if (attempt !== generation.current) return;
          try {
            const path = await open({
              multiple: false,
              filters: [{ name: "Codex account", extensions: ["json"] }],
            });
            if (typeof path !== "string" || attempt !== generation.current) {
              if (attempt === generation.current) void start(mode);
              return;
            }
            const account = await routingUpstreamsApi.importCodexAccount(path);
            await client.invalidateQueries({
              queryKey: ["managed-auth-status", "codex_oauth"],
            });
            if (attempt === generation.current) onComplete(account.id);
          } catch {
            if (attempt === generation.current) setError("codexBrowser.failed");
          }
        })();
      }}
    />
  );
}
function NativeSignIn({
  provider,
  onCancel,
  onComplete,
}: Props & { provider: NativeSubscriptionProvider }) {
  const login = useNativeSubscriptionLogin(provider);
  const client = useQueryClient();
  const [proxy, setProxy] = useState("");
  const [actionError, setActionError] = useState<string>();
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    void login.start();
    return () => {
      mounted.current = false;
    };
  }, [login.start]);
  useCompletion(login.completion, onComplete);
  const name =
    subscriptionProviders.find((p) => p.id === provider)?.name ?? provider;
  return (
    <CodexLoginPanel
      providerName={name}
      mode="browser"
      proxy={proxy}
      url={login.flow?.url}
      starting={login.starting}
      openFailed={login.openFailed}
      waitingMessage={
        provider === "claude" ? "nativeSubscription.claudeWaiting" : undefined
      }
      error={login.error ?? actionError}
      submitting={login.submitting}
      submitted={login.submitted}
      onCancel={() => {
        void login.cancel();
        onCancel();
      }}
      onReopen={() => void login.reopen()}
      onCopy={() => {
        if (login.flow?.url)
          void copyText(login.flow.url).catch(() =>
            setActionError("nativeSubscription.copyFailed"),
          );
      }}
      onSubmit={
        provider !== "claude" ? (value) => void login.submit(value) : undefined
      }
      onProxy={(value) => {
        setProxy(value);
        setActionError(undefined);
        void login.start(undefined, value);
      }}
      onRetry={
        login.error ? () => void login.start(undefined, proxy) : undefined
      }
      onImport={
        provider !== "chatgpt_api"
          ? () => {
              void (async () => {
                await login.cancel();
                try {
                  const path = await open({
                    multiple: false,
                    filters: [{ name: "Account JSON", extensions: ["json"] }],
                  });
                  if (!mounted.current) return;
                  if (typeof path !== "string") {
                    void login.start(undefined, proxy);
                    return;
                  }
                  const account = await nativeSubscriptionApi.import(
                    provider,
                    path,
                  );
                  await client.invalidateQueries({
                    queryKey: ["native-subscription-accounts", provider],
                  });
                  if (mounted.current) onComplete(account.id);
                } catch {
                  if (mounted.current)
                    setActionError("nativeSubscription.failed");
                }
              })();
            }
          : undefined
      }
    />
  );
}
function DeviceSignIn({
  provider,
  onCancel,
  onComplete,
}: Props & { provider: "github_copilot" | "xai_oauth" }) {
  const auth = useManagedAuth(provider);
  const [error, setError] = useState<string>();
  useEffect(() => {
    auth.addAccount();
    return () => {
      void auth.cancelAuth();
    };
  }, []);
  useCompletion(auth.loginCompletion?.accountId, onComplete);
  const url = auth.deviceCode?.verification_uri;
  const name =
    subscriptionProviders.find((p) => p.id === provider)?.name ?? provider;
  return (
    <CodexLoginPanel
      providerName={name}
      mode="device"
      proxy=""
      url={url}
      code={auth.deviceCode?.user_code}
      starting={!auth.deviceCode && !auth.error}
      waitingMessage="subscriptionProvider.deviceHint"
      error={error ?? (auth.error ? "codexBrowser.failed" : undefined)}
      onCancel={() => {
        void auth.cancelAuth();
        onCancel();
      }}
      onReopen={() => {
        if (url)
          void settingsApi
            .openExternal(url)
            .catch(() => setError("codexBrowser.openFailed"));
      }}
      onCopy={() => {
        if (url)
          void copyText(url).catch(() =>
            setError("nativeSubscription.copyFailed"),
          );
      }}
      onCodeCopy={() => {
        if (auth.deviceCode)
          void copyText(auth.deviceCode.user_code).catch(() =>
            setError("nativeSubscription.copyFailed"),
          );
      }}
      onRetry={auth.error ? () => auth.addAccount() : undefined}
    />
  );
}
