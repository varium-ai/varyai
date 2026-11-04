// Layout and interactions adapted from the reference implementation's routing plugin (MIT).
// See third_party/routing-reference/LICENSE. The stage consumes recorded gateway decisions.
import {
  ArrowDown,
  ChevronDown,
  Eye,
  EyeOff,
  GitBranch,
  Pause,
  Play,
  Plus,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { EditingState } from "@/hooks/useWorkspaceEditing";
import { IntentRoutingPanel } from "./IntentRoutingPanel";
import { RoutingPurposeFilter } from "./RoutingPurposeFilter";
import { RoutingAccounts } from "./RoutingAccounts";
import { RoutingStage } from "./RoutingStage";
import { routingPurpose } from "@/lib/routingPurposes";
import { routingModelLabel } from "@/lib/routingModelLabel";
import { AccountLabel, AccountMask } from "@/components/AccountLabel";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import * as authApi from "@/lib/api/auth";
import { routingTraceApi, type RoutingTrace } from "@/lib/api/routingTrace";
import "./routing.css";

type Row = RoutingTrace;
const metricKeys = [
  "duration",
  "ttft",
  "tokens",
  "cache",
  "speed",
  "cost",
] as const;
type Metric = (typeof metricKeys)[number];

function stored<T>(key: string, fallback: T, parse: (s: string) => T): T {
  try {
    const value = localStorage.getItem(key);
    return value === null ? fallback : parse(value);
  } catch {
    return fallback;
  }
}
function persist(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preferences are optional when local storage is unavailable.
  }
}
function live(row?: Row) {
  return row?.state === "routing" || row?.state === "streaming";
}
function rerouted(row: Row) {
  return row.steps.filter((step) => step.kind === "attempt").length > 1;
}
function answered(row: Row) {
  return [...row.steps].reverse().find((step) => step.kind === "attempt")
    ?.candidate;
}
function seconds(ms?: number | null) {
  return ms == null ? "\u2014" : `${(ms / 1000).toFixed(ms < 1000 ? 2 : 1)}s`;
}

export function RoutingPage({
  onEditingChange,
}: {
  onEditingChange?: (state: EditingState) => void;
}) {
  const { t, i18n } = useTranslation();
  const [masked, setMasked] = useState(false);
  const logins = useQuery({
    queryKey: ["managed-auth-status", "codex_oauth"],
    queryFn: () => authApi.authGetStatus("codex_oauth"),
    enabled: masked,
    staleTime: 30000,
  });
  const [bySession, setBySession] = useState(() =>
    stored("vary.routingBySession", false, (value) => value === "1"),
  );
  const [metrics, setMetrics] = useState<Metric[]>(() =>
    stored("vary.routingMetrics", [...metricKeys], (value) => {
      const parsed: unknown = JSON.parse(value);
      return Array.isArray(parsed)
        ? metricKeys.filter((key) => parsed.includes(key))
        : [...metricKeys];
    }),
  );
  const [day, setDay] = useState("live");
  const [purposes, setPurposes] = useState<string[]>([]);
  const [selection, setSelection] = useState<Row | null>(null);
  const [createRequest, setCreateRequest] = useState(0);
  const [replay, setReplay] = useState(false);
  const [playhead, setPlayhead] = useState(0);
  const [speed, setSpeed] = useState(1);
  const groups = useRef<HTMLDivElement>(null);
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
  const dates = trace.data?.days ?? [];
  const history = useQuery({
    queryKey: ["routing-history", day],
    queryFn: () => routingTraceApi.history(day),
    enabled: day !== "live",
    refetchIntervalInBackground: false,
  });
  const allRows: Row[] =
    day === "live"
      ? running
        ? (trace.data?.requests ?? [])
        : []
      : (history.data?.requests ?? []);
  const rows = useMemo(
    () =>
      allRows.filter(
        (row) =>
          !purposes.length || purposes.includes(routingPurpose(row.kind)),
      ),
    [allRows, purposes],
  );
  const selected =
    (selection && rows.find((row) => row.id === selection.id)) ?? rows[0];
  const activeSteps = selected
    ? selected.steps.filter((step) => !replay || step.atMs <= playhead)
    : [];
  const shown = selected
    ? {
        ...selected,
        steps: activeSteps,
        ...(replay && playhead < selected.durationMs
          ? { state: "routing" as const }
          : {}),
      }
    : undefined;
  const replayEnded = !!selected && playhead >= selected.durationMs;
  useEffect(() => {
    if (!replay || !selected || replayEnded) return;
    const timer = window.setInterval(() => {
      setPlayhead((previous) => {
        const next = Math.min(previous + 100 * speed, selected.durationMs);
        return next;
      });
    }, 100);
    return () => window.clearInterval(timer);
  }, [replay, replayEnded, speed, selected?.id, selected?.durationMs]);

  const accounts = trace.data?.accounts ?? [];
  const accountName = (candidate: {
    providerId: string;
    providerName: string;
  }) => candidate.providerName;
  const grouped = useMemo(() => {
    const sessions = new Map<string, Row[]>();
    for (const row of rows) {
      const key = bySession && row.session ? row.session : row.id;
      sessions.set(key, [...(sessions.get(key) ?? []), row]);
    }
    return [...sessions.entries()];
  }, [rows, bySession]);
  function pick(row: Row) {
    setSelection(row);
    setReplay(false);
    setPlayhead(0);
  }
  function lookAt(value: string) {
    setDay(value);
    setSelection(null);
    setReplay(false);
  }
  function metric(row: Row, key: Metric): string | undefined {
    if (key === "duration")
      return !live(row) && row.durationMs > 0
        ? seconds(row.durationMs)
        : undefined;
    if (key === "ttft")
      return row.firstTokenMs != null && row.firstTokenMs > 0
        ? seconds(row.firstTokenMs)
        : undefined;
    if (key === "tokens")
      return row.inputTokens == null && row.outputTokens == null
        ? undefined
        : `${row.inputTokens?.toLocaleString() ?? "\u2014"} \u2191 ${row.outputTokens?.toLocaleString() ?? "\u2014"} \u2193`;
    if (key === "cache") {
      const total = (row.inputTokens ?? 0) + (row.cacheReadTokens ?? 0);
      return row.cacheReadTokens == null || !total
        ? undefined
        : `${Math.round((row.cacheReadTokens / total) * 100)}%`;
    }
    if (key === "speed")
      return row.outputTokensPerSecond != null && row.outputTokensPerSecond > 0
        ? `${row.outputTokensPerSecond.toFixed(1)}/s`
        : undefined;
    if (key === "cost" && row.costUsd != null) {
      const cost = Number(row.costUsd);
      if (Number.isFinite(cost) && cost >= 0) return `$${cost.toFixed(4)}`;
    }
    // A trace without a recorded price must not imply a free request.
    return undefined;
  }
  const dateFormat = new Intl.DateTimeFormat(i18n.language, {
    month: "short",
    day: "numeric",
  });
  return (
    <AccountMask
      masked={masked}
      names={logins.data?.accounts.map((account) => account.login)}
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
        <button
          className="text"
          onClick={() =>
            groups.current?.scrollIntoView({
              behavior: "smooth",
              block: "start",
            })
          }
        >
          <ArrowDown size={13} />
          {t("intentRouting.title")}
        </button>
        <button
          className="text"
          onClick={() => {
            setCreateRequest((n) => n + 1);
            groups.current?.scrollIntoView({
              behavior: "smooth",
              block: "start",
            });
          }}
        >
          <Plus size={13} />
          {t("intentRouting.create")}
        </button>
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
                  day === "live" && trace.data && !running
                    ? "routingPage.gatewayStopped"
                    : "routingPage.waiting",
                )}
              </b>
            )}
            {selection && (
              <button className="text" onClick={() => lookAt("live")}>
                {t("routingPage.followLive")}
              </button>
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
        {replay && selected && (
          <div className="rt-replay">
            <span>{t("routingPage.replaying")}</span>
            <b>{seconds(playhead)}</b>
            <span>/ {seconds(selected.durationMs)}</span>
            <span className="grow" />
            <button
              className="text"
              onClick={() => setSpeed(speed === 4 ? 1 : speed * 2)}
            >
              {speed}x
            </button>
            <button className="text" onClick={() => setReplay(false)}>
              <Pause size={12} />
              {t("routingPage.stop")}
            </button>
            <input
              aria-label={t("routingPage.playhead")}
              type="range"
              min={0}
              max={selected.durationMs}
              value={playhead}
              onChange={(event) => setPlayhead(Number(event.target.value))}
            />
          </div>
        )}
        <RoutingStage
          row={shown}
          accountName={accountName}
          animate={replay || (running && day === "live" && !selection)}
          replay={replay}
          speed={replay ? speed : 1}
          requestedModel={
            shown
              ? routingModelLabel(shown, modelCatalog.data ?? []) ||
                t("routingPage.modelUnavailable")
              : undefined
          }
          address={address}
          currentAccounts={accounts}
          concurrent={running && day === "live" && !replay ? allRows : []}
          liveObservations={running && day === "live" && !replay}
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
            <button
              onClick={() => {
                const failed = rows.find((row) => row.state === "failed");
                if (failed) pick(failed);
              }}
              disabled={!rows.some((row) => row.state === "failed")}
            >
              <b>{rows.filter((row) => row.state === "failed").length}</b>
              {t("routingPage.errors")}
            </button>
          </div>
        </div>
        {shown && (
          <div className="rt-log">
            <div className="rt-log-head">
              <time dateTime={new Date(shown.startedAt).toISOString()}>
                {new Date(shown.startedAt).toLocaleTimeString(i18n.language)}
              </time>
              <span>{shown.agent}</span>
              {requestedModel(shown)}
              <span className="grow" />
              {shown.durationMs > 0 && <span>{seconds(shown.durationMs)}</span>}
              {!live(shown) && shown.steps.length > 0 && !replay && (
                <button
                  className="text"
                  onClick={() => {
                    setSelection(selected ?? null);
                    setPlayhead(0);
                    setReplay(true);
                  }}
                >
                  <Play size={12} />
                  {t("routingPage.replay")}
                </button>
              )}
            </div>
            {
              <ol className="rt-steps" aria-label={t("routingPage.decisions")}>
                {activeSteps.map((step, index) => {
                  const candidate =
                    step.candidate != null
                      ? shown.candidates[step.candidate]
                      : null;
                  return (
                    <li
                      key={index}
                      className={
                        ["rejected", "failed", "cancelled"].includes(step.kind)
                          ? "bad"
                          : step.kind === "completed"
                            ? "ok"
                            : step.kind === "attempt"
                              ? "why"
                              : ""
                      }
                    >
                      <span>
                        {t(`routingPage.step.${step.kind}`, {
                          strategy: t(`routingPage.strategy.${shown.strategy}`),
                        })}
                      </span>
                      {candidate && (
                        <span className="rt-step-target">
                          <AccountLabel>{accountName(candidate)}</AccountLabel>{" "}
                          <code>
                            {candidate.model}
                            {candidate.fixedEffort
                              ? ` :${candidate.fixedEffort}`
                              : ""}
                          </code>
                        </span>
                      )}
                      {step.status != null && (
                        <code className="rt-status">{step.status}</code>
                      )}
                      <time>+{seconds(step.atMs)}</time>
                    </li>
                  );
                })}
              </ol>
            }
            {shown.estimatedTokens != null && (
              <div className="rt-context">
                <span>{t("routingPage.contextEstimate")}</span>
                <b>{shown.estimatedTokens.toLocaleString()}</b>
                {shown.candidates[answered(shown) ?? 0]?.contextWindow !=
                  null && (
                  <span>
                    /{" "}
                    {shown.candidates[
                      answered(shown) ?? 0
                    ].contextWindow?.toLocaleString()}
                  </span>
                )}
              </div>
            )}
          </div>
        )}
      </section>
      <div className={`rt-cols ${!rows.length ? "solo" : ""}`}>
        <section className="rt-col">
          <div className="row-head">
            <h2 className="label">{t("routingPage.requests")}</h2>
            <span className="note">{rows.length}</span>
            <span className="grow" />
            <RoutingPurposeFilter
              purposes={allRows.map((row) => routingPurpose(row.kind))}
              selected={purposes}
              onChange={(next) => {
                setPurposes(next);
                setSelection(null);
                setReplay(false);
              }}
            />
            <details className="rt-metrics">
              <summary>
                {t("routingPage.metrics")}
                <ChevronDown size={11} />
              </summary>
              <div className="rt-metric-menu">
                {metricKeys.map((key) => (
                  <label key={key}>
                    <input
                      type="checkbox"
                      checked={metrics.includes(key)}
                      onChange={() => {
                        const next = metricKeys.filter((metricKey) =>
                          metricKey === key
                            ? !metrics.includes(key)
                            : metrics.includes(metricKey),
                        );
                        setMetrics(next);
                        persist("vary.routingMetrics", JSON.stringify(next));
                      }}
                    />
                    {t(`routingPage.metric.${key}`)}
                  </label>
                ))}
              </div>
            </details>
          </div>
          <div className="rt-filters">
            <div className="rt-days">
              <button
                className={`rt-day ${day === "live" ? "on" : ""}`}
                aria-pressed={day === "live"}
                onClick={() => lookAt("live")}
              >
                {t("routingPage.live")}
              </button>
              {dates.slice(0, 2).map((date) => (
                <button
                  key={date}
                  className={`rt-day ${day === date ? "on" : ""}`}
                  aria-pressed={day === date}
                  onClick={() => lookAt(date)}
                >
                  {dateFormat.format(new Date(`${date}T12:00:00`))}
                </button>
              ))}
              {dates.length > 2 && (
                <details className="rt-earlier">
                  <summary
                    className={`rt-day ${day !== "live" && !dates.slice(0, 2).includes(day) ? "on" : ""}`}
                  >
                    {t("routingPage.earlier")}
                    <ChevronDown size={10} />
                  </summary>
                  <div className="rt-earlier-menu">
                    {dates.slice(2).map((date) => (
                      <button
                        key={date}
                        className={`rt-day ${day === date ? "on" : ""}`}
                        aria-pressed={day === date}
                        onClick={(event) => {
                          lookAt(date);
                          event.currentTarget
                            .closest("details")
                            ?.removeAttribute("open");
                        }}
                      >
                        {dateFormat.format(new Date(`${date}T12:00:00`))}
                      </button>
                    ))}
                  </div>
                </details>
              )}
            </div>
            <div className="rt-group-by">
              {[false, true].map((value) => (
                <button
                  key={String(value)}
                  className={`rt-day ${bySession === value ? "on" : ""}`}
                  aria-pressed={bySession === value}
                  onClick={() => {
                    setBySession(value);
                    persist("vary.routingBySession", value ? "1" : "0");
                  }}
                >
                  {t(value ? "routingPage.bySession" : "routingPage.byRequest")}
                </button>
              ))}
            </div>
          </div>
          <div
            className="list rt-reqs"
            aria-busy={day === "live" ? trace.isFetching : history.isFetching}
          >
            {(day !== "live" ? history.isError : trace.data?.historyError) && (
              <p role="alert" className="none">
                {t("routingPage.historyFailed")}
              </p>
            )}
            {(day === "live" ? trace.isPending : history.isPending) &&
            !rows.length ? (
              <p className="none" role="status">
                {t("common.loading")}
              </p>
            ) : (
              !rows.length && (
                <p className="none">
                  {t(
                    day === "live" && !running
                      ? "routingPage.gatewayStopped"
                      : "routingPage.noRequests",
                  )}
                  <small className="rt-empty-hint">
                    {t("routingPage.gatewayOnlyHint")}
                  </small>
                </p>
              )
            )}
            {grouped.map(([key, requests], index) => (
              <div
                key={key}
                className={bySession && requests[0].session ? "rt-session" : ""}
              >
                {bySession && requests[0].session && (
                  <div className="rt-session-head">
                    <GitBranch size={12} />
                    <span>{t("routingPage.session", { n: index + 1 })}</span>
                    <small>{requests.length}</small>
                  </div>
                )}
                {requests.map((row) => {
                  const destination = answered(row);
                  const target =
                    destination != null
                      ? row.candidates[destination]
                      : undefined;
                  return (
                    <button
                      key={row.id}
                      className={`rt-req ${selected?.id === row.id ? "on" : ""} ${row.state === "failed" ? "bad" : ""}`}
                      aria-pressed={selected?.id === row.id}
                      onClick={() => pick(row)}
                    >
                      <time dateTime={new Date(row.startedAt).toISOString()}>
                        {new Date(row.startedAt).toLocaleTimeString(
                          i18n.language,
                          {
                            hour: "2-digit",
                            minute: "2-digit",
                            second: "2-digit",
                          },
                        )}
                      </time>
                      <span className="rt-req-title">
                        <span
                          className={`dot ${live(row) ? "now" : ""}`}
                          aria-hidden
                        />
                        <b>{row.agent}</b>
                        {requestedModel(row)}
                        {rerouted(row) && <GitBranch size={11} />}
                      </span>
                      <span className="rt-req-target">
                        {target ? (
                          <>
                            <AccountLabel>{accountName(target)}</AccountLabel>
                            <span>{"\u00b7"}</span>
                            <code>{target.model}</code>
                          </>
                        ) : (
                          t(`routingPage.state.${row.state}`)
                        )}
                      </span>
                      <span className="meta">
                        {metrics.map((key) => {
                          const value = metric(row, key);
                          return value === undefined ? null : (
                            <span
                              key={key}
                              title={t(`routingPage.metric.${key}`)}
                            >
                              <small>
                                {t(`routingPage.metricShort.${key}`)}
                              </small>
                              {value}
                            </span>
                          );
                        })}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
          {day !== "live" && history.data?.cut && (
            <p className="rt-history-cut">
              {t("routingPage.historyCut", { n: 2000 })}
            </p>
          )}
        </section>
        {rows.length > 0 && (
          <RoutingAccounts
            requests={
              replay && selected
                ? rows.map((row) =>
                    row.id === selected.id
                      ? { ...row, steps: activeSteps }
                      : row,
                  )
                : rows
            }
            current={accounts}
            live={day === "live" && running && !replay}
            refresh={() => void trace.refetch()}
            refreshing={trace.isFetching}
            accountName={accountName}
          />
        )}
      </div>
      <div ref={groups} className="rt-groups">
        <IntentRoutingPanel
          createRequest={createRequest}
          maskAccounts={masked}
          onEditingChange={onEditingChange}
        />
      </div>
    </AccountMask>
  );
}
