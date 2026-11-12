import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  nativeSubscriptionApi as api,
  type NativeSubscriptionProvider,
  type SubscriptionLogin,
} from "@/lib/api/nativeSubscription";
import { settingsApi } from "@/lib/api/settings";

const safeError = (cause: unknown) =>
  typeof cause === "string" &&
  [
    "nativeSubscription.expired",
    "nativeSubscription.clientRequired",
    "nativeSubscription.claudeMissing",
    "nativeSubscription.cursorRuntime",
    "nativeSubscription.cursorProxyUnsupported",
    "nativeSubscription.identityMismatch",
    "nativeSubscription.callbackInvalid",
    "nativeSubscription.proxyInvalid",
  ].includes(cause)
    ? cause
    : "nativeSubscription.failed";

export function useNativeSubscriptionLogin(
  provider: NativeSubscriptionProvider,
) {
  const client = useQueryClient();
  const [flow, setFlow] = useState<SubscriptionLogin>();
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string>();
  const [openFailed, setOpenFailed] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [completion, setCompletion] = useState<string>();
  const generation = useRef(0);
  const active = useRef<string>();
  const mounted = useRef(true);
  const cancel = useCallback(async () => {
    generation.current += 1;
    const id = active.current;
    active.current = undefined;
    if (mounted.current) {
      setFlow(undefined);
      setStarting(false);
      setError(undefined);
      setCompletion(undefined);
      setSubmitted(false);
      setSubmitting(false);
    }
    if (id) await api.cancel(id).catch(() => {});
  }, []);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      void cancel();
    };
  }, [cancel]);
  const reopen = useCallback(async (url?: string) => {
    if (!url) return;
    const attempt = generation.current;
    try {
      await settingsApi.openExternal(url);
      if (mounted.current && attempt === generation.current)
        setOpenFailed(false);
    } catch {
      if (mounted.current && attempt === generation.current)
        setOpenFailed(true);
    }
  }, []);
  const start = useCallback(
    async (target?: string, proxy?: string) => {
      const previous = active.current;
      active.current = undefined;
      const attempt = ++generation.current;
      setStarting(true);
      setError(undefined);
      setFlow(undefined);
      setCompletion(undefined);
      setSubmitted(false);
      setSubmitting(false);
      setOpenFailed(false);
      try {
        if (previous) await api.cancel(previous);
        if (!mounted.current || attempt !== generation.current) return;
        const next =
          proxy === undefined
            ? await api.start(provider, target)
            : await api.start(provider, target, proxy);
        if (!mounted.current || attempt !== generation.current) {
          await api.cancel(next.id);
          return;
        }
        active.current = next.id;
        setFlow(next);
        setStarting(false);
        if (next.url) await reopen(next.url);
      } catch (cause) {
        if (mounted.current && attempt === generation.current) {
          setStarting(false);
          setError(safeError(cause));
        }
      }
    },
    [provider, reopen],
  );
  useEffect(() => {
    if (!flow || flow.state !== "waiting") return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const attempt = generation.current;
    const poll = async () => {
      try {
        const next = await api.poll(flow.id);
        if (stopped || attempt !== generation.current) return;
        if (next.state === "done" && next.account) {
          await client.invalidateQueries({
            queryKey: ["native-subscription-accounts", provider],
          });
          if (stopped || attempt !== generation.current) return;
          active.current = undefined;
          setFlow(next);
          setCompletion(next.account.id);
        } else if (next.state === "failed") {
          active.current = undefined;
          setFlow(next);
          setError(safeError(next.error));
        } else timer = setTimeout(() => void poll(), 1000);
      } catch (cause) {
        if (stopped || attempt !== generation.current) return;
        void api.cancel(flow.id).catch(() => {});
        active.current = undefined;
        setFlow(undefined);
        setError(safeError(cause));
      }
    };
    timer = setTimeout(() => void poll(), 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [flow?.id, flow?.state, provider, client]);
  const submit = useCallback(async (url: string) => {
    const id = active.current;
    if (!id) return;
    const attempt = generation.current;
    setSubmitting(true);
    setError(undefined);
    try {
      await api.callback(id, url);
      if (mounted.current && attempt === generation.current) setSubmitted(true);
    } catch (cause) {
      if (mounted.current && attempt === generation.current)
        setError(safeError(cause));
    } finally {
      if (mounted.current && attempt === generation.current)
        setSubmitting(false);
    }
  }, []);
  return {
    flow,
    starting,
    error,
    openFailed,
    completion,
    submitting,
    submitted,
    start,
    cancel,
    reopen: () => reopen(flow?.url),
    submit,
  };
}
