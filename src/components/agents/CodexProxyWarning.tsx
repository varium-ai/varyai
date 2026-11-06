import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import {
  agentConnectionsApi,
  type CodexProxyIssue,
} from "@/lib/api/agentConnections";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";

const diagnosticKey = ["codex-proxy-diagnostic"] as const;
let requestSequence = 0;

export interface CodexProxyWarningProps {
  enabled: boolean;
  busy: boolean;
  suspended?: boolean;
  onDisconnect: () => void;
  onIssueChange?: (issue: CodexProxyIssue | null) => void;
}

export function CodexProxyWarning({
  enabled,
  busy,
  suspended = false,
  onDisconnect,
  onIssueChange,
}: CodexProxyWarningProps) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const forceNextCheck = useRef(false);
  const eligibility = useRef({ enabled, afterRequest: requestSequence });
  if (eligibility.current.enabled !== enabled) {
    eligibility.current = { enabled, afterRequest: requestSequence };
  }
  const diagnostic = useQuery({
    queryKey: diagnosticKey,
    queryFn: async ({ signal }) => {
      const request = ++requestSequence;
      const force = forceNextCheck.current;
      forceNextCheck.current = false;
      const issue = await agentConnectionsApi.diagnoseCodexProxy(force);
      signal.throwIfAborted();
      return { issue, request };
    },
    enabled,
    refetchInterval: 30000,
    retry: false,
  });
  // Cached or late results from before this connection became eligible cannot
  // establish a current issue. The command itself only returns sanitized data.
  const currentResult =
    enabled &&
    diagnostic.isSuccess &&
    diagnostic.data.request > eligibility.current.afterRequest;
  const issue = currentResult ? diagnostic.data.issue : null;

  useEffect(() => {
    if (!enabled) {
      void client.cancelQueries({ queryKey: diagnosticKey, exact: true });
    }
  }, [client, enabled]);
  useEffect(() => {
    onIssueChange?.(issue);
    return () => onIssueChange?.(null);
  }, [issue, onIssueChange]);
  useEffect(() => {
    if (!enabled || suspended || (currentResult && !issue)) setOpen(false);
  }, [enabled, suspended, currentResult, issue]);

  const checking = diagnostic.isFetching;
  const dialogOpen =
    enabled && !suspended && open && (!!issue || diagnostic.isError);
  return (
    <>
      {issue && (
        <button
          type="button"
          className="mp-agent-fix"
          disabled={busy || suspended}
          aria-label={t("agentConnections.codexProxy.howToFix")}
          title={t("agentConnections.codexProxy.status")}
          onClick={() => setOpen(true)}
        >
          <AlertTriangle size={11} aria-hidden="true" />
          <span>{t("agentConnections.codexProxy.howToFix")}</span>
        </button>
      )}
      <AppDialog
        open={dialogOpen}
        onOpenChange={setOpen}
        size="lg"
        title={t("agentConnections.codexProxy.title")}
        description={t("agentConnections.codexProxy.description")}
        footer={
          <div className="flex w-full flex-wrap justify-end gap-2">
            <Button
              variant="outline"
              disabled={busy || checking}
              onClick={() => {
                forceNextCheck.current = true;
                void diagnostic.refetch();
              }}
            >
              {t(
                checking
                  ? "agentConnections.codexProxy.checking"
                  : "agentConnections.codexProxy.checkAgain",
              )}
            </Button>
            <Button
              variant="outline"
              disabled={busy || checking || !issue}
              onClick={() => {
                setOpen(false);
                onDisconnect();
              }}
            >
              {t("agentConnections.disconnectRestore")}
            </Button>
            <Button onClick={() => setOpen(false)}>
              {t("agentConnections.codexProxy.acknowledge")}
            </Button>
          </div>
        }
      >
        {issue ? (
          <div className="space-y-4 text-sm leading-relaxed">
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-md border p-3 text-xs">
              <dt>{t("agentConnections.codexProxy.target")}</dt>
              <dd className="break-all font-mono">{issue.target}</dd>
              <dt>{t("agentConnections.codexProxy.proxy")}</dt>
              <dd className="break-all font-mono">{issue.proxy}</dd>
              <dt>{t("agentConnections.codexProxy.source")}</dt>
              <dd>
                {t(
                  issue.source === "environment"
                    ? "agentConnections.codexProxy.sourceEnvironment"
                    : "agentConnections.codexProxy.sourceSystem",
                )}
              </dd>
            </dl>
            <p className="text-muted-foreground">
              {t("agentConnections.codexProxy.proof")}
            </p>
            <ol className="list-decimal space-y-3 pl-5">
              <li>{t("agentConnections.codexProxy.proxyRules")}</li>
              <li>
                <p>{t("agentConnections.codexProxy.environment")}</p>
                <p className="mt-2 text-xs text-muted-foreground">
                  {t("agentConnections.codexProxy.macos")}
                </p>
                <p className="mt-2 text-xs text-muted-foreground">
                  {t("agentConnections.codexProxy.windows")}
                </p>
              </li>
              <li>{t("agentConnections.codexProxy.reopen")}</li>
            </ol>
            <p className="text-xs text-muted-foreground">
              {t("agentConnections.codexProxy.manualChanges")}
            </p>
          </div>
        ) : (
          <p role="status" className="text-sm text-muted-foreground">
            {t("agentConnections.codexProxy.unavailable")}
          </p>
        )}
      </AppDialog>
    </>
  );
}
