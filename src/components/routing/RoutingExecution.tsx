import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  routingExecutionApi,
  type RoutingExecutionSession,
  type RoutingGroup,
} from "@/lib/api/intentRouting";

export function RoutingExecution({ group }: { group: RoutingGroup }) {
  const { t } = useTranslation();
  const [consent, setConsent] = useState(false);
  const [pending, setPending] = useState(false);
  const [session, setSession] = useState<RoutingExecutionSession | null>(null);
  const [error, setError] = useState(false);
  const active = useRef(false);
  const token = useRef<string | null>(null);
  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      if (token.current) {
        void routingExecutionApi.stop(token.current).catch(() => {});
        token.current = null;
      }
    };
  }, []);
  async function toggle() {
    setPending(true);
    setError(false);
    try {
      if (session) {
        await routingExecutionApi.stop(session.token);
        token.current = null;
        if (!active.current) return;
        setSession(null);
        setConsent(false);
      } else {
        const created = await routingExecutionApi.start(group, consent);
        if (!active.current) {
          await routingExecutionApi.stop(created.token);
          return;
        }
        token.current = created.token;
        setSession(created);
      }
    } catch {
      if (active.current) setError(true);
    } finally {
      if (active.current) setPending(false);
    }
  }
  const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
  const command = session
    ? [
        `VARYAI_ROUTING_TOKEN=${quote(session.token)} codex`,
        `-c ${quote('model_provider="varyai_session"')}`,
        `-c ${quote(`model=${JSON.stringify(session.model)}`)}`,
        `-c ${quote(`model_catalog_json=${JSON.stringify(session.modelCatalogPath)}`)}`,
        `-c ${quote('model_providers.varyai_session.name="VaryAI routing session"')}`,
        `-c ${quote(`model_providers.varyai_session.base_url=${JSON.stringify(session.baseUrl)}`)}`,
        `-c ${quote('model_providers.varyai_session.env_key="VARYAI_ROUTING_TOKEN"')}`,
        `-c ${quote('model_providers.varyai_session.wire_api="responses"')}`,
      ].join(" \\\n  ")
    : "";
  return (
    <details className="rounded-lg border border-border p-3">
      <summary className="cursor-pointer text-sm font-medium">
        {t("intentRouting.executionTitle")}
      </summary>
      <p className="my-3 text-sm text-muted-foreground">
        {t("intentRouting.executionNotice")}
      </p>
      {!session && (
        <label className="mb-3 flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={consent}
            disabled={pending}
            onChange={(e) => setConsent(e.target.checked)}
          />
          {t("intentRouting.executionConsent")}
        </label>
      )}
      <Button
        type="button"
        variant="outline"
        disabled={pending || (!session && !consent)}
        onClick={() => void toggle()}
      >
        {t(
          session
            ? "intentRouting.executionStop"
            : "intentRouting.executionStart",
        )}
      </Button>
      {error && (
        <p role="alert" className="mt-2 text-sm text-destructive">
          {t("intentRouting.executionFailed")}
        </p>
      )}
      {session && (
        <>
          <p className="mt-3 text-sm text-muted-foreground">
            {t("intentRouting.executionCommand")}
          </p>
          <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-all rounded-md bg-muted p-3 text-xs select-all">
            {command}
          </pre>
        </>
      )}
    </details>
  );
}
