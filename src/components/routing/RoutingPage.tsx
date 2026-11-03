// Layout and interactions adapted from the reference implementation's routing plugin (MIT).
// See third_party/routing-reference/LICENSE. The stage consumes recorded gateway decisions.
import {
  ArrowDown,
  ArrowRight,
  Check,
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
import { RoutingPackets } from "./RoutingPackets";
import { routingPurpose } from "@/lib/routingPurposes";
import { routingModelLabel } from "@/lib/routingModelLabel";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import {
  routingTraceApi,
  type RoutingCandidate,
  type RoutingTrace,
  type RoutingTraceStep,
} from "@/lib/api/routingTrace";
import { usageApi } from "@/lib/api/usage";
import type { RequestLog } from "@/types/usage";
import { APP_ICON_NAME, APP_IDS } from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import appIcon from "@/assets/icons/app-icon.png";
import "./routing.css";

type Row = RoutingTrace & { archived?: boolean; cost?: string };
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
function dayKey(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
function archivedRow(log: RequestLog): Row {
  return {
    id: log.requestId,
    startedAt: log.createdAt * 1000,
    agent: log.appType,
    model: log.requestModel || log.model,
    groupName: "",
    strategy: "",
    state: log.statusCode < 400 ? "completed" : "failed",
    status: log.statusCode,
    durationMs: log.latencyMs,
    firstTokenMs: log.firstTokenMs,
    inputTokens: log.inputTokens,
    outputTokens: log.outputTokens,
    cacheReadTokens: log.cacheReadTokens,
    candidates: [
      {
        providerId: log.providerId,
        providerName: log.providerName || log.providerId,
        model: log.model,
      },
    ],
    steps: [],
    archived: true,
    cost: log.totalCostUsd,
  };
}

export function RoutingPage({
  onEditingChange,
}: {
  onEditingChange?: (state: EditingState) => void;
}) {
  const { t, i18n } = useTranslation();
  const [masked, setMasked] = useState(false);
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
  const [page, setPage] = useState(0);
  const [createRequest, setCreateRequest] = useState(0);
  const [replay, setReplay] = useState(false);
  const [playhead, setPlayhead] = useState(0);
  const [speed, setSpeed] = useState(1);
  const groups = useRef<HTMLDivElement>(null);
  const trace = useQuery({
    queryKey: ["routing-trace"],
    queryFn: routingTraceApi.snapshot,
    refetchInterval: day === "live" ? 1000 : false,
    refetchIntervalInBackground: false,
  });
  const modelCatalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
    staleTime: 30000,
  });
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
  const dates = useMemo(
    () =>
      Array.from({ length: 4 }, (_, index) => {
        const date = new Date();
        date.setDate(date.getDate() - index);
        return dayKey(date);
      }),
    [],
  );
  const range = useMemo(() => {
    if (day === "live") return {};
    const start = new Date(`${day}T00:00:00`);
    const end = new Date(start);
    end.setDate(end.getDate() + 1);
    return {
      startDate: Math.floor(start.getTime() / 1000),
      endDate: Math.floor(end.getTime() / 1000) - 1,
    };
  }, [day]);
  const history = useQuery({
    queryKey: ["routing-history", day, page],
    queryFn: () =>
      usageApi.getRequestLogs(range, day === "live" ? 0 : page, 80),
    refetchInterval: day === "live" ? 10000 : false,
  });
  const allRows = useMemo<Row[]>(() => {
    const recent =
      trace.data?.requests.filter(
        (row) => day === "live" || dayKey(new Date(row.startedAt)) === day,
      ) ?? [];
    const logs = history.data?.data ?? [];
    const costs = new Map(logs.map((log) => [log.requestId, log.totalCostUsd]));
    const ids = new Set(
      (day === "live" || page === 0 ? recent : []).map((row) => row.id),
    );
    return [
      ...(day === "live" || page === 0
        ? recent.map((row) => ({ ...row, cost: costs.get(row.id) }))
        : []),
      ...logs.filter((log) => !ids.has(log.requestId)).map(archivedRow),
    ].sort((a, b) => b.startedAt - a.startedAt);
  }, [trace.data, history.data, day, page]);
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
  const names = useMemo(() => {
    const ids = [
      ...accounts.map((account) => account.providerId),
      ...(trace.data?.requests ?? []).flatMap((row) =>
        row.candidates.map((candidate) => candidate.providerId),
      ),
      ...rows.flatMap((row) =>
        row.candidates.map((candidate) => candidate.providerId),
      ),
      ...(selected?.candidates ?? []).map((candidate) => candidate.providerId),
    ];
    return new Map(
      [...new Set(ids)].sort().map((id, index) => [id, index + 1]),
    );
  }, [accounts, trace.data, rows, selected]);
  const accountName = (candidate: {
    providerId: string;
    providerName: string;
  }) =>
    masked
      ? t("routingPage.maskedAccount", { n: names.get(candidate.providerId) })
      : candidate.providerName;
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
    setPage(0);
    setSelection(null);
    setReplay(false);
  }
  function metric(row: Row, key: Metric) {
    if (key === "duration") return seconds(row.durationMs);
    if (key === "ttft") return seconds(row.firstTokenMs);
    if (key === "tokens")
      return row.inputTokens == null
        ? "\u2014"
        : `${row.inputTokens.toLocaleString()} \u2191 ${row.outputTokens?.toLocaleString() ?? "\u2014"} \u2193`;
    if (key === "cache") {
      const total = (row.inputTokens ?? 0) + (row.cacheReadTokens ?? 0);
      return row.cacheReadTokens == null || !total
        ? "\u2014"
        : `${Math.round((row.cacheReadTokens / total) * 100)}%`;
    }
    if (key === "speed")
      return row.outputTokensPerSecond != null
        ? `${row.outputTokensPerSecond.toFixed(1)}/s`
        : "\u2014";
    return row.cost != null && Number.isFinite(Number(row.cost))
      ? `$${Number(row.cost).toFixed(4)}`
      : "\u2014";
  }
  const dateFormat = new Intl.DateTimeFormat(i18n.language, {
    month: "short",
    day: "numeric",
  });
  return (
    <div className="vary-routing" data-testid="routing-page">
      <header className="row-head rt-page-head">
        <h1 className="label">{t("workspaceNav.routing")}</h1>
        <span
          className={`rt-livedot ${trace.isError ? "off" : ""}`}
          aria-hidden
        />
        <span className="note">{t("routingPage.liveHint")}</span>
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
                <b>{shown.agent}</b>
                <ArrowRight size={12} />
                {requestedModel(shown)}
              </>
            ) : (
              t("routingPage.waiting")
            )}
            {selection && (
              <button className="text" onClick={() => lookAt("live")}>
                {t("routingPage.followLive")}
              </button>
            )}
          </div>
          <p className="rt-mode">
            {shown?.strategy
              ? t(`routingPage.strategyHelp.${shown.strategy}`, {
                  defaultValue: t(`routingPage.strategy.${shown.strategy}`),
                })
              : t("routingPage.stageHint")}
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
          animate={replay || (day === "live" && !selection)}
          replay={replay}
          speed={replay ? speed : 1}
        />
        <div className="rt-foot">
          <p className="rt-cap" aria-live="polite">
            {shown
              ? t(`routingPage.state.${shown.state}`)
              : t("routingPage.emptyStage")}
          </p>
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
              <span>{seconds(shown.durationMs)}</span>
              {!shown.archived &&
                !live(shown) &&
                shown.steps.length > 0 &&
                !replay && (
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
            {shown.archived ? (
              <p className="rt-archive-note">{t("routingPage.summaryOnly")}</p>
            ) : (
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
                          {accountName(candidate)}{" "}
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
            )}
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
      <div className="rt-cols">
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
              <details className="rt-earlier">
                <summary
                  className={`rt-day ${day !== "live" && !dates.slice(0, 2).includes(day) ? "on" : ""}`}
                >
                  {t("routingPage.earlier")}
                  <ChevronDown size={10} />
                </summary>
                <label>
                  {t("routingPage.chooseDay")}
                  <input
                    type="date"
                    max={dates[0]}
                    value={day === "live" ? dates[0] : day}
                    onChange={(event) => {
                      if (event.target.value) lookAt(event.target.value);
                    }}
                  />
                </label>
              </details>
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
          <div className="list rt-reqs" aria-busy={history.isFetching}>
            {history.isError && (
              <p role="alert" className="none">
                {t("routingPage.historyFailed")}
              </p>
            )}
            {trace.isPending && !rows.length ? (
              <p className="none" role="status">
                {t("common.loading")}
              </p>
            ) : (
              !rows.length && (
                <p className="none">{t("routingPage.noRequests")}</p>
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
                  const target = row.candidates[answered(row) ?? 0];
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
                            {accountName(target)}
                            <span>{"\u00b7"}</span>
                            <code>{target.model}</code>
                          </>
                        ) : (
                          t("routingPage.state.routing")
                        )}
                      </span>
                      <span className="meta">
                        {metrics.map((key) => (
                          <span
                            key={key}
                            title={t(`routingPage.metric.${key}`)}
                          >
                            <small>{t(`routingPage.metricShort.${key}`)}</small>
                            {metric(row, key)}
                          </span>
                        ))}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
          {day !== "live" && (history.data?.total ?? 0) > 80 && (
            <div className="rt-pager">
              <button
                className="text"
                disabled={page === 0 || history.isFetching}
                onClick={() => {
                  setSelection(null);
                  setPage(page - 1);
                }}
              >
                {t("routingPage.previous")}
              </button>
              <span>
                {page + 1} / {Math.ceil((history.data?.total ?? 0) / 80)}
              </span>
              <button
                className="text"
                disabled={
                  (page + 1) * 80 >= (history.data?.total ?? 0) ||
                  history.isFetching
                }
                onClick={() => {
                  setSelection(null);
                  setPage(page + 1);
                }}
              >
                {t("routingPage.next")}
              </button>
            </div>
          )}
        </section>
        <RoutingAccounts
          requests={
            replay && selected
              ? rows.map((row) =>
                  row.id === selected.id ? { ...row, steps: activeSteps } : row,
                )
              : rows
          }
          current={accounts}
          live={day === "live" && !replay}
          refresh={() => void trace.refetch()}
          refreshing={trace.isFetching}
          accountName={accountName}
        />
      </div>
      <div ref={groups} className="rt-groups">
        <IntentRoutingPanel
          createRequest={createRequest}
          maskAccounts={masked}
          onEditingChange={onEditingChange}
        />
      </div>
    </div>
  );
}

function RoutingStage({
  row,
  accountName,
  animate,
  replay,
  speed,
}: {
  row?: Row;
  accountName: (candidate: RoutingCandidate) => string;
  animate: boolean;
  replay: boolean;
  speed: number;
}) {
  const { t } = useTranslation();
  const stage = useRef<HTMLDivElement>(null);
  const agent = APP_IDS.find((id) => id === row?.agent);
  const [paths, setPaths] = useState<string[]>([]);
  const [narrow, setNarrow] = useState(false);
  const chosen = row?.archived ? 0 : row ? answered(row) : undefined;
  useEffect(() => {
    const node = stage.current;
    if (!node) return;
    function layout() {
      if (!node) return;
      const box = node.getBoundingClientRect();
      setNarrow(box.width <= 560);
      const source = node.querySelector(".rt-src")?.getBoundingClientRect();
      const hub = node.querySelector(".rt-hub")?.getBoundingClientRect();
      const candidates = [...node.querySelectorAll(".rt-accts li")].map(
        (child) => child.getBoundingClientRect(),
      );
      if (!source || !hub) return;
      const wire = (a: DOMRect, b: DOMRect) => {
        if (b.top >= a.bottom) {
          const x1 = a.left + a.width / 2 - box.left;
          const y1 = a.bottom - box.top;
          const x2 = b.left - box.left;
          const y2 = b.top + b.height / 2 - box.top;
          return `M${x1},${y1} C${x1},${y1 + 12} ${x2 - 12},${y1 + 12} ${x2 - 12},${y2 - 8} Q${x2 - 12},${y2} ${x2},${y2}`;
        }
        const x1 = a.right - box.left,
          y1 = a.top + a.height / 2 - box.top;
        const x2 = b.left - box.left,
          y2 = b.top + b.height / 2 - box.top;
        const middle = (x1 + x2) / 2;
        return `M${x1},${y1} C${middle},${y1} ${middle},${y2} ${x2},${y2}`;
      };
      setPaths([
        wire(source, hub),
        ...candidates.map((candidate) => wire(hub, candidate)),
      ]);
    }
    layout();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(layout);
    observer.observe(node);
    for (const child of node.querySelectorAll(".rt-node,.rt-accts"))
      observer.observe(child);
    return () => observer.disconnect();
  }, [row?.id, row?.candidates.length, narrow]);
  function lastStep(index: number): RoutingTraceStep | undefined {
    return row?.steps.filter((step) => step.candidate === index).at(-1);
  }
  return (
    <div className={`rt-stage ${narrow ? "narrow" : ""}`} ref={stage}>
      <svg className="rt-wires" aria-hidden>
        {paths.map((path, index) => (
          <path
            key={index}
            d={path}
            className={
              row && (index === 0 || index - 1 === chosen)
                ? live(row)
                  ? "live"
                  : "lit"
                : ""
            }
          />
        ))}
      </svg>
      <RoutingPackets
        row={row?.archived ? undefined : row}
        paths={paths}
        active={animate}
        replay={replay}
        speed={speed}
      />
      <div className="rt-srcs">
        <div className={`rt-node rt-src ${live(row) ? "now" : ""}`}>
          <span className="rt-agent-icon">
            {agent ? (
              <ProviderIcon
                icon={APP_ICON_NAME[agent]}
                name={agent}
                size={26}
              />
            ) : (
              row?.agent.charAt(0).toUpperCase() || "A"
            )}
          </span>
          <b>{row?.agent ?? t("routingPage.agent")}</b>
          <small>{t("routingPage.source")}</small>
        </div>
      </div>
      <div className="rt-node rt-hub">
        <img src={appIcon} className="rt-brand-icon" alt="" />
        <b>Vary AI</b>
        <small>127.0.0.1</small>
        <i>
          {row?.groupName === "Direct model"
            ? t("routingPage.directModel")
            : row?.groupName || t("routingPage.gateway")}
        </i>
      </div>
      <ol className="rt-accts">
        {!row?.candidates.length && (
          <li className="idle">{t("routingPage.emptyCandidates")}</li>
        )}
        {row?.candidates.map((candidate, index) => {
          const step = lastStep(index);
          const rejected = ["rejected", "context", "images", "failed"].includes(
            step?.kind ?? "",
          );
          return (
            <li
              key={`${candidate.providerId}/${candidate.model}/${index}`}
              className={`${chosen === index ? "on" : ""} ${chosen === index && live(row) ? "now" : ""} ${rejected ? "rest" : ""}`}
            >
              <span className="dot" />
              <b>
                {accountName(candidate)}
                <code className="mdl">
                  {candidate.model}
                  {candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""}
                </code>
              </b>
              <em>
                {step?.status && step.status >= 400 ? (
                  step.status
                ) : chosen === index ? (
                  row.state === "completed" ? (
                    <Check
                      size={13}
                      aria-label={t("routingPage.state.completed")}
                    />
                  ) : (
                    t(`routingPage.state.${row.state}`)
                  )
                ) : (
                  t(
                    step
                      ? `routingPage.step.${step.kind}`
                      : "routingPage.standby",
                  )
                )}
              </em>
              {candidate.contextWindow != null && (
                <small className="rt-window">
                  {t("routingPage.window", {
                    size: `${Math.round(candidate.contextWindow / 1000)}k`,
                  })}
                </small>
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}
