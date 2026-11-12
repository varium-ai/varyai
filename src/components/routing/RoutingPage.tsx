// Layout and interactions adapted from the reference implementation's routing plugin (MIT).
// See third_party/routing-reference/LICENSE. The stage consumes recorded gateway decisions.
import { Eye, EyeOff, Plus } from "lucide-react";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { EditingState } from "@/hooks/useWorkspaceEditing";
import { IntentRoutingPanel } from "./IntentRoutingPanel";
import { RoutingStage } from "./RoutingStage";
import { routingModelLabel } from "@/lib/routingModelLabel";
import { AccountMask } from "@/components/AccountLabel";
import { Button } from "@/components/ui/button";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import * as authApi from "@/lib/api/auth";
import { routingTraceApi, type RoutingTrace } from "@/lib/api/routingTrace";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import "./routing.css";

type Row = RoutingTrace;
function live(row?: Row) {
  return row?.state === "routing" || row?.state === "streaming";
}
function rerouted(row: Row) {
  return row.steps.filter((step) => step.kind === "attempt").length > 1;
}
export function RoutingPage({
  onEditingChange,
}: {
  onEditingChange?: (state: EditingState) => void;
}) {
  const { t } = useTranslation();
  const [masked, setMasked] = useState(false);
  const logins = useQuery({
    queryKey: ["managed-auth-status", "codex_oauth"],
    queryFn: () => authApi.authGetStatus("codex_oauth"),
    enabled: masked,
    staleTime: 30000,
  });
  const [createRequest, setCreateRequest] = useState(0);
  const trace = useQuery({
    queryKey: ["routing-trace"],
    queryFn: routingTraceApi.snapshot,
    refetchInterval: 1000,
    refetchIntervalInBackground: false,
  });
  const modelCatalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
    staleTime: 30000,
  });
  const listener = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  const address =
    listener.data?.running && listener.data.port > 0
      ? `${listener.data.address.includes(":") ? `[${listener.data.address}]` : listener.data.address}:${listener.data.port}`
      : undefined;
  function requestedModel(row: Row) {
    const label =
      routingModelLabel(row, modelCatalog.data ?? []) ||
      t("routingPage.modelUnavailable");
    return (
      <code
        className="rt-model-label"
        title={label !== row.model ? row.model : undefined}
      >
        {label}
      </code>
    );
  }
  const running = trace.data?.running === true;
  const rows: Row[] = running ? (trace.data?.requests ?? []) : [];
  const upstreams = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    enabled: masked || rows.some((row) => row.candidates.length > 0),
    staleTime: 30000,
  });
  const shown = rows[0];
  const focusedRequests = rows.filter(
    (row) =>
      row.agent === shown?.agent &&
      row.model === shown.model &&
      row.session === shown.session,
  );
  const accounts = trace.data?.accounts ?? [];
  const accountName = (candidate: {
    providerId: string;
    providerName: string;
  }) => candidate.providerName;
  return (
    <AccountMask
      masked={masked}
      names={[
        ...(upstreams.data?.map((provider) => provider.name) ?? []),
        ...(logins.data?.accounts.map((account) => account.login) ?? []),
        ...rows.flatMap((row) =>
          row.candidates.flatMap((candidate) =>
            candidate.account?.label ? [candidate.account.label] : [],
          ),
        ),
      ]}
      className="vary-routing"
      data-testid="routing-page"
    >
      <header className="row-head rt-page-head">
        <h1 className="label">{t("workspaceNav.routing")}</h1>
        <span
          className={`rt-livedot ${!running || trace.isError ? "off" : ""}`}
          aria-hidden
        />
        <span className="note">
          {t(
            trace.data && !running
              ? "routingPage.gatewayStopped"
              : "routingPage.liveHint",
          )}
        </span>
        <span className="grow" />
        <Button size="sm" onClick={() => setCreateRequest((n) => n + 1)}>
          <Plus size={13} />
          {t("intentRouting.create")}
        </Button>
        <button
          className="text rt-mask"
          aria-pressed={masked}
          onClick={() => setMasked(!masked)}
        >
          {masked ? <Eye size={14} /> : <EyeOff size={14} />}
          {t(masked ? "routingPage.showAccounts" : "routingPage.hideAccounts")}
        </button>
      </header>
      {trace.isError && (
        <p role="alert" className="rt-error">
          {t("routingPage.failed")}
        </p>
      )}
      <section className="rt" aria-label={t("routingPage.stage")}>
        <div className="rt-top">
          <div className="rt-what">
            {shown ? (
              <>
                {requestedModel(shown)}
                {shown.candidates.length > 0 && (
                  <span className="rt-route-summary">
                    {t("routingPage.seatsOn", { n: shown.candidates.length })}
                  </span>
                )}
              </>
            ) : (
              <b>
                {t(
                  trace.data && !running
                    ? "routingPage.gatewayStopped"
                    : "routingPage.waiting",
                )}
              </b>
            )}
          </div>
          <p className="rt-mode">
            {shown && !shown.candidates.length
              ? t(
                  live(shown)
                    ? "routingPage.planningCandidates"
                    : "routingPage.noCandidatesRecorded",
                )
              : shown?.candidates.length === 1
                ? t("routingPage.singleSeat")
                : shown?.strategy
                  ? t(`routingPage.strategyHelp.${shown.strategy}`, {
                      defaultValue: t(`routingPage.strategy.${shown.strategy}`),
                    })
                  : t("routingPage.emptyStage")}
          </p>
        </div>
        <RoutingStage
          row={shown}
          upstreams={upstreams.data}
          accountName={accountName}
          animate={running}
          replay={false}
          speed={1}
          requestedModel={
            shown
              ? routingModelLabel(shown, modelCatalog.data ?? []) ||
                t("routingPage.modelUnavailable")
              : undefined
          }
          address={address}
          currentAccounts={accounts}
          concurrent={focusedRequests}
          liveObservations={running}
        />
        <div className={`rt-foot${shown ? "" : " idle"}`}>
          {shown && (
            <p className="rt-cap" aria-live="polite">
              {t(`routingPage.state.${shown.state}`)}
            </p>
          )}
          <div className="rt-stats">
            <span>
              <b>{rows.length}</b>
              {t("routingPage.requests")}
            </span>
            <span>
              <b>{rows.filter(rerouted).length}</b>
              {t("routingPage.rerouted")}
            </span>
            <span>
              <b>{rows.filter((row) => row.state === "failed").length}</b>
              {t("routingPage.errors")}
            </span>
          </div>
        </div>
      </section>
      <div className="rt-groups">
        <IntentRoutingPanel
          createRequest={createRequest}
          showCreateButton={false}
          maskAccounts={masked}
          onEditingChange={onEditingChange}
        />
      </div>
    </AccountMask>
  );
}
