import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  codexBrowserLoginApi as api,
  type CodexBrowserLogin,
} from "@/lib/api/codexBrowserLogin";
import { settingsApi } from "@/lib/api/settings";

const safeError = (error: unknown) =>
  typeof error === "string" &&
  [
    "codexBrowser.callbackInvalid",
    "codexBrowser.proxyInvalid",
    "codexBrowser.expired",
    "codexBrowser.duplicate",
    "codexBrowser.tokenRejected",
    "codexBrowser.networkFailed",
    "codexBrowser.exchangeTimeout",
    "codexBrowser.invalidTokenResponse",
    "codexBrowser.saveFailed",
    "codexBrowser.identityMismatch",
  ].includes(error)
    ? error
    : "codexBrowser.failed";
export function useCodexBrowserLogin() {
  const client = useQueryClient();
  const [flow, setFlow] = useState<CodexBrowserLogin>();
  const [starting, setStarting] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [error, setError] = useState<string>();
  const [openFailed, setOpenFailed] = useState(false);
  const [completion, setCompletion] = useState<{
    accountId: string;
    generation: number;
  }>();
  const generation = useRef(0);
  const current = useRef<string>();
  const mounted = useRef(true);
  const cancel = useCallback(async () => {
    generation.current += 1;
    const id = current.current;
    current.current = undefined;
    if (mounted.current) {
      setFlow(undefined);
      setStarting(false);
      setSubmitting(false);
      setSubmitted(false);
      setError(undefined);
    }
    if (id) {
      try {
        if (!(await api.cancel(id)))
          await client.invalidateQueries({
            queryKey: ["managed-auth-status", "codex_oauth"],
          });
      } catch {
        await client.invalidateQueries({
          queryKey: ["managed-auth-status", "codex_oauth"],
        });
      }
    }
  }, [client]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      void cancel();
    };
  }, [cancel]);
  const start = useCallback(
    async (target?: string, proxy?: string) => {
      const previous = current.current;
      current.current = undefined;
      const attempt = ++generation.current;
      setStarting(true);
      setSubmitting(false);
      setSubmitted(false);
      setError(undefined);
      setFlow(undefined);
      setOpenFailed(false);
      setCompletion(undefined);
      try {
        if (previous) await api.cancel(previous);
        if (!mounted.current || attempt !== generation.current) return;
        const next = await api.start(target, proxy);
        if (!mounted.current || attempt !== generation.current) {
          await api.cancel(next.id);
          return;
        }
        current.current = next.id;
        setFlow(next);
        setStarting(false);
        try {
          await settingsApi.openExternal(next.url);
        } catch {
          if (mounted.current && attempt === generation.current)
            setOpenFailed(true);
        }
      } catch (cause) {
        if (mounted.current && attempt === generation.current) {
          setError(safeError(cause));
          setStarting(false);
        }
      }
    },
    [cancel],
  );
  useEffect(() => {
    if (!flow || flow.status !== "waiting") return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const attempt = generation.current;
    const poll = async () => {
      try {
        const next = await api.poll(flow.id);
        if (stopped || attempt !== generation.current) return;
        if (next.status === "done" && next.account) {
          current.current = undefined;
          await client.invalidateQueries({
            queryKey: ["managed-auth-status", "codex_oauth"],
          });
          if (stopped || attempt !== generation.current) return;
          setCompletion({ accountId: next.account.id, generation: attempt });
          setFlow(undefined);
        } else if (next.status === "failed") {
          current.current = undefined;
          setError(safeError(next.error));
          setFlow(next);
        } else {
          timer = setTimeout(() => void poll(), 1000);
        }
      } catch (cause) {
        if (stopped || attempt !== generation.current) return;
        setError(safeError(cause));
        setFlow((old) => (old ? { ...old, status: "failed" } : old));
      }
    };
    timer = setTimeout(() => void poll(), 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [flow?.id, flow?.status, client]);
  const submit = useCallback(async (url: string) => {
    const id = current.current;
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
  const reopen = useCallback(async () => {
    if (!flow?.url) return;
    const attempt = generation.current;
    try {
      await settingsApi.openExternal(flow.url);
      if (attempt === generation.current) setOpenFailed(false);
    } catch {
      if (attempt === generation.current) setOpenFailed(true);
    }
  }, [flow?.url]);
  return {
    flow,
    starting,
    submitting,
    submitted,
    error,
    openFailed,
    completion,
    start,
    cancel,
    submit,
    reopen,
    isPending: starting || flow?.status === "waiting",
  };
}
