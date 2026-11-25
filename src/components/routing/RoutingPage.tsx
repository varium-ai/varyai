// Layout and interactions adapted from the reference implementation's routing plugin (MIT).
// See third_party/routing-reference/LICENSE. The stage consumes recorded gateway decisions.
import { Eye, EyeOff, Plus } from "lucide-react";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { EditingState } from "@/hooks/useWorkspaceEditing";
import { IntentRoutingPanel } from "./IntentRoutingPanel";
import { RoutingStage } from "./RoutingStage";
import {
  liveRoutingSources,
  routeInProgress,
  routingSourceKey,
} from "./useRoutingFlights";
import { routingModelLabel } from "@/lib/routingModelLabel";
import { routingPurpose } from "@/lib/routingPurposes";
import { AccountMask } from "@/components/AccountLabel";
import { Button } from "@/components/ui/button";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import * as authApi from "@/lib/api/auth";
import { routingTraceApi, type RoutingTrace } from "@/lib/api/routingTrace";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import "./routing.css";

type Row = RoutingTrace;
const backgroundPurposes = new Set([
  "kind:thread_title",
  "kind:thread_description",
  "kind:memory_consolidation",
  "kind:composer_predictions",
  "kind:ambient_suggestions",
  "kind:guardian",
  "kind:provider_test",
  "kind:api_detect",
  "kind:model_list",
]);
function rerouted(row: Row) {
  return row.steps.filter((step) => step.kind === "attempt").length > 1;
}
export function RoutingPage({
  onEditingChange,
  onAddProvider,
}: {
  onEditingChange?: (state: EditingState) => void;
  onAddProvider?: () => void;
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
  const modelLabel = (row: Row) =>
    routingModelLabel(row, modelCatalog.data ?? []) ||
    t("routingPage.modelUnavailable");
  function requestedModel(row: Row) {
    const label = modelLabel(row);
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
  const stageRows = rows.filter(
    (row) => !backgroundPurposes.has(routingPurpose(row.kind)),
  );
  const sources = liveRoutingSources(stageRows, trace.dataUpdatedAt);
  const shown = sources[0];
  const continuation = shown?.steps.find(
    ({ kind }) => kind === "pinned" || kind === "affinity",
  );
  const sourceIds = new Set(sources.map(routingSourceKey));
  const concurrent = stageRows.filter((row) =>
    sourceIds.has(routingSourceKey(row)),
  );
  const multipleRoutes =
    sources.length > 1 ||
    concurrent.some(
      (row) =>
        routeInProgress(row) &&
        (row.model !== shown?.model || row.session !== shown.session),
    );
  const accounts = trace.data?.accounts ?? [];
  const accountName = (candidate: {
    providerId: string;
    providerName: string;
  }) => candidate.providerName;
  const maskLabel = t(
    masked ? "routingPage.showAccounts" : "routingPage.hideAccounts",
  );
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
        <div className="rt-page-copy">
          <h1 className="label vary-page-title">{t("workspaceNav.routing")}</h1>
          <p className="note rt-page-note">
            <span
              className={`rt-livedot ${!running || trace.isError ? "off" : ""}`}
              aria-hidden
            />
            {t(
              trace.data && !running
                ? "routingPage.gatewayStopped"
                : "routingPage.liveHint",
            )}
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateRequest((n) => n + 1)}>
          <Plus size={13} />
          {t("intentRouting.create")}
        </Button>
      </header>
      {trace.isError && (
        <p role="alert" className="rt-error">
          {t("routingPage.failed")}
        </p>
      )}
      <section className="rt" aria-label={t("routingPage.stage")}>
        <div className="rt-top">
          <div className="rt-what">
            {multipleRoutes ? (
              <b>{t("routingPage.stage")}</b>
            ) : shown ? (
              requestedModel(shown)
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
          {!multipleRoutes && (
            <p className="rt-mode">
              {shown && !shown.candidates.length
                ? t(
                    routeInProgress(shown)
                      ? "routingPage.planningCandidates"
                      : "routingPage.noCandidatesRecorded",
                  )
                : continuation
                  ? t(`routingPage.step.${continuation.kind}`)
                  : shown?.candidates.length === 1
                    ? t("routingPage.singleSeat")
                    : shown
                      ? t("routingPage.candidateSeats")
                      : t("routingPage.emptyStage")}
            </p>
          )}
        </div>
        <RoutingStage
          row={shown}
          upstreams={upstreams.data}
          accountName={accountName}
          animate={running}
          replay={false}
          speed={1}
          modelLabel={modelLabel}
          address={address}
          currentAccounts={accounts}
          concurrent={concurrent}
          sources={sources}
          liveObservations={running}
        />
        <div className={`rt-foot${shown ? "" : " idle"}`}>
          {shown && !multipleRoutes && (
            <p className="rt-cap" aria-live="polite">
              {t(`routingPage.state.${shown.state}`)}
              {shown.classificationOutcome &&
                ` · ${t(`routingPage.classificationOutcome.${shown.classificationOutcome}`)}`}
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
          toolbarActions={
            <Button
              variant="ghost"
              size="sm"
              className="rt-mask"
              aria-label={maskLabel}
              title={maskLabel}
              aria-pressed={masked}
              onClick={() => setMasked(!masked)}
            >
              {masked ? (
                <Eye size={16} aria-hidden="true" />
              ) : (
                <EyeOff size={16} aria-hidden="true" />
              )}
              <span className="rt-group-action-label">{maskLabel}</span>
            </Button>
          }
          onEditingChange={onEditingChange}
          onAddProvider={onAddProvider}
        />
      </div>
    </AccountMask>
  );
}
