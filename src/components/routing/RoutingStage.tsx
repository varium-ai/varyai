// Adapted from the reference routing stage (MIT), without credential-derived labels.
// See third_party/routing-reference/LICENSE.
import { Bot } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { APP_ICON_NAME, APP_IDS } from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  UpstreamIcon,
  upstreamIcon,
} from "@/components/providers/UpstreamIcon";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import { AccountLabel, useAccountText } from "@/components/AccountLabel";
import appIcon from "@/assets/icons/app-icon.png";
import { RoutingPackets } from "./RoutingPackets";
import { useRoutingFlights } from "./useRoutingFlights";
import type {
  RoutingAccount,
  RoutingCandidate,
  RoutingTrace,
} from "@/lib/api/routingTrace";

function live(row?: RoutingTrace) {
  return row?.state === "routing" || row?.state === "streaming";
}
const hues: Record<string, string> = {
  claude: "#d97757",
  "claude-desktop": "#d97757",
  codex: "#7f83d8",
  gemini: "#0ea5e9",
  opencode: "#c69a14",
  pi: "#10b981",
};
const seat = (c: RoutingCandidate) =>
  c.account?.id
    ? `${c.account.id}\0${c.model}\0${c.fixedEffort ?? ""}`
    : undefined;
const candidateKey = (candidate: RoutingCandidate) =>
  JSON.stringify([
    candidate.providerId,
    candidate.account?.kind,
    candidate.account?.id,
    candidate.model,
    candidate.fixedEffort,
  ]);
const chosenCandidate = (trace?: RoutingTrace) =>
  [...(trace?.steps ?? [])].reverse().find((step) => step.kind === "attempt")
    ?.candidate;
function isAnswering(trace?: RoutingTrace) {
  const chosen = chosenCandidate(trace);
  return (
    live(trace) &&
    chosen != null &&
    ![
      "rejected",
      "failed",
      "context",
      "images",
      "first_token_timeout",
      "cancelled",
    ].includes(
      [...(trace?.steps ?? [])]
        .reverse()
        .find((step) => step.candidate === chosen)?.kind ?? "",
    )
  );
}
export function RoutingStage({
  row,
  accountName,
  animate,
  replay,
  speed,
  requestedModel,
  address,
  currentAccounts = [],
  upstreams = [],
  concurrent = [],
  liveObservations = false,
  layout: stageLayout = "responsive",
  now = Date.now(),
}: {
  row?: RoutingTrace;
  accountName: (candidate: RoutingCandidate) => string;
  animate: boolean;
  replay: boolean;
  speed: number;
  requestedModel?: string;
  address?: string;
  currentAccounts?: RoutingAccount[];
  upstreams?: RoutingUpstream[];
  concurrent?: RoutingTrace[];
  liveObservations?: boolean;
  layout?: "responsive" | "compact";
  now?: number;
}) {
  const { t } = useTranslation();
  const accountText = useAccountText();
  const stage = useRef<HTMLDivElement>(null);
  const { flights, finish } = useRoutingFlights(
    row,
    concurrent,
    animate && !replay,
  );
  const traces = row
    ? [...flights.filter((trace) => trace.id !== row.id), row]
    : flights;
  const gatewayWorking = animate && !replay && traces.some(live);
  const agents = [...new Set(traces.map((trace) => trace.agent))];
  const sourceAgents: (string | undefined)[] = agents.length
    ? agents
    : [undefined];
  const seats = new Map<
    string,
    { candidate: RoutingCandidate; trace: RoutingTrace; index: number }
  >();
  for (const trace of traces) {
    trace.candidates.forEach((candidate, index) => {
      const key = candidateKey(candidate);
      const previous = seats.get(key);
      if (
        !previous ||
        (isAnswering(trace) &&
          chosenCandidate(trace) === index &&
          (!isAnswering(previous.trace) ||
            chosenCandidate(previous.trace) !== previous.index)) ||
        (!live(previous.trace) && live(trace))
      )
        seats.set(key, { candidate, trace, index });
    });
  }
  const candidates = [...seats.values()];
  const candidateIndex = new Map(
    candidates.map(({ candidate }, index) => [candidateKey(candidate), index]),
  );
  const [paths, setPaths] = useState<{ sources: string[]; targets: string[] }>({
    sources: [],
    targets: [],
  });
  const [narrow, setNarrow] = useState(false);
  const layoutKey = JSON.stringify([
    sourceAgents,
    candidates.map(({ candidate }) => candidateKey(candidate)),
  ]);
  const busy = new Set(
    concurrent
      .filter((r) => r.id !== row?.id && live(r))
      .flatMap((r) => {
        const index = [...r.steps]
          .reverse()
          .find((step) => step.kind === "attempt")?.candidate;
        const last = [...r.steps]
          .reverse()
          .find((step) => step.candidate === index);
        if (
          index == null ||
          !r.candidates[index] ||
          !["attempt", "connected"].includes(last?.kind ?? "")
        )
          return [];
        const id = seat(r.candidates[index]);
        return id ? [id] : [];
      }),
  );
  useEffect(() => {
    const node = stage.current;
    if (!node) return;
    function layout() {
      if (!node) return;
      const box = node.getBoundingClientRect();
      setNarrow(stageLayout === "responsive" && box.width <= 560);
      const sources = [...node.querySelectorAll(".rt-src")].map((source) =>
        source.getBoundingClientRect(),
      );
      const sourceList = node
        .querySelector(".rt-srcs")
        ?.getBoundingClientRect();
      const hub = node.querySelector(".rt-hub")?.getBoundingClientRect();
      const list = node.querySelector(".rt-accts")?.getBoundingClientRect();
      const candidates = [
        ...node.querySelectorAll(".rt-accts li:not(.idle)"),
      ].map((child) => child.getBoundingClientRect());
      if (!sources.length || !hub) return;
      const wire = (a: DOMRect, b: DOMRect) => {
        if (b.left < a.right) {
          const x1 = a.left + a.width / 2 - box.left,
            y1 = a.bottom - box.top;
          const x2 = b.left - box.left,
            y2 = b.top + b.height / 2 - box.top;
          return `M${x1},${y1} C${x1},${y1 + 12} ${x2 - 12},${y1 + 12} ${x2 - 12},${y2 - 8} Q${x2 - 12},${y2} ${x2},${y2}`;
        }
        const x1 = a.right - box.left,
          y1 = a.top + a.height / 2 - box.top;
        const x2 = b.left - box.left,
          y2 = b.top + b.height / 2 - box.top;
        const middle = (x1 + x2) / 2;
        return `M${x1},${y1} C${middle},${y1} ${middle},${y2} ${x2},${y2}`;
      };
      setPaths({
        sources: sources.map((source) =>
          sourceList &&
          (source.bottom <= sourceList.top || source.top >= sourceList.bottom)
            ? ""
            : wire(source, hub),
        ),
        targets: candidates.map((candidate) =>
          list && (candidate.bottom <= list.top || candidate.top >= list.bottom)
            ? ""
            : wire(hub, candidate),
        ),
      });
    }
    layout();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(layout);
    observer.observe(node);
    for (const child of node.querySelectorAll(
      ".rt-node,.rt-srcs,.rt-accts,.rt-accts li",
    ))
      observer.observe(child);
    const lists = node.querySelectorAll(".rt-srcs,.rt-accts");
    lists.forEach((list) =>
      list.addEventListener("scroll", layout, { passive: true }),
    );
    return () => {
      observer.disconnect();
      lists.forEach((list) => list.removeEventListener("scroll", layout));
    };
  }, [layoutKey, narrow, stageLayout]);
  const emptyKey = !row
    ? "emptyCandidates"
    : row.state === "failed"
      ? "noCandidatesFailed"
      : row.state === "cancelled"
        ? "noCandidatesCancelled"
        : live(row)
          ? "planningCandidates"
          : "noCandidatesRecorded";
  const hubDescription = [
    "Vary AI",
    address || t("routingPage.listenerUnknown"),
    row && row.candidates.length > 1
      ? t(`routingPage.strategy.${row.strategy}`)
      : "",
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <div
      className={`rt-stage${narrow ? " narrow" : ""}${stageLayout === "compact" ? " compact" : ""}`}
      ref={stage}
      style={
        {
          "--rt-agent": hues[row?.agent ?? ""] ?? "var(--rt-accent)",
        } as CSSProperties
      }
    >
      <svg className="rt-wires" aria-hidden>
        {[...paths.sources, ...paths.targets].map((path, index) => (
          <path
            key={index}
            d={path}
            className={(() => {
              const matching = traces.filter((trace) => {
                if (index < paths.sources.length)
                  return sourceAgents[index] === trace.agent;
                const chosen = chosenCandidate(trace);
                const target =
                  chosen == null ? undefined : trace.candidates[chosen];
                return (
                  target &&
                  candidateIndex.get(candidateKey(target)) ===
                    index - paths.sources.length
                );
              });
              return matching.some((trace) => isAnswering(trace)) && animate
                ? "live"
                : matching.some(
                      (trace) =>
                        trace.state === "completed" || isAnswering(trace),
                    )
                  ? "lit"
                  : "";
            })()}
          />
        ))}
      </svg>
      <RoutingPackets
        row={row}
        paths={[
          paths.sources[Math.max(0, sourceAgents.indexOf(row?.agent))] ?? "",
          ...(row?.candidates.map(
            (candidate) =>
              paths.targets[
                candidateIndex.get(candidateKey(candidate)) ?? -1
              ] ?? "",
          ) ?? []),
        ]}
        flights={
          animate && !replay
            ? flights.map((trace, flightIndex) => ({
                row: trace,
                color: hues[trace.agent] ?? "var(--rt-accent)",
                holdOffset:
                  flights.slice(0, flightIndex).filter((other) => {
                    const chosen = chosenCandidate(trace);
                    const otherChosen = chosenCandidate(other);
                    const target =
                      chosen == null ? undefined : trace.candidates[chosen];
                    const otherTarget =
                      otherChosen == null
                        ? undefined
                        : other.candidates[otherChosen];
                    return target && otherTarget
                      ? candidateKey(target) === candidateKey(otherTarget)
                      : !target && !otherTarget && trace.agent === other.agent;
                  }).length * 12,
                paths: [
                  paths.sources[sourceAgents.indexOf(trace.agent)] ?? "",
                  ...trace.candidates.map(
                    (candidate) =>
                      paths.targets[
                        candidateIndex.get(candidateKey(candidate)) ?? -1
                      ] ?? "",
                  ),
                ],
              }))
            : undefined
        }
        onFinished={finish}
        active={animate}
        replay={replay}
        speed={speed}
      />
      <div className="rt-srcs">
        {sourceAgents.map((source) => {
          const agent = APP_IDS.find((id) => id === source);
          const name = agent
            ? agent === "claude"
              ? t("apps.claudeCode")
              : t(`apps.${agent}`)
            : source || t("routingPage.agent");
          const selectedSource = source === row?.agent;
          const sourceTrace = traces.find((trace) => trace.agent === source);
          return (
            <div
              key={source ?? "idle"}
              className={`rt-node rt-src ${traces.some((trace) => trace.agent === source && isAnswering(trace)) ? "now" : ""}`}
              style={
                {
                  "--rt-agent": hues[source ?? ""] ?? "var(--rt-accent)",
                } as CSSProperties
              }
              title={selectedSource ? requestedModel : sourceTrace?.model}
            >
              <span className="rt-agent-icon">
                {agent ? (
                  <ProviderIcon
                    icon={APP_ICON_NAME[agent]}
                    name={name}
                    size={stageLayout === "compact" ? 18 : 26}
                  />
                ) : (
                  <Bot
                    size={stageLayout === "compact" ? 18 : 26}
                    aria-hidden="true"
                  />
                )}
              </span>
              <b>{name}</b>
              {row && selectedSource && requestedModel && (
                <small
                  className={stageLayout === "compact" ? "sr-only" : undefined}
                  title={row?.model !== requestedModel ? row?.model : undefined}
                >
                  {requestedModel}
                </small>
              )}
            </div>
          );
        })}
      </div>
      <div
        className={`rt-node rt-hub${gatewayWorking ? " working" : ""}`}
        title={hubDescription}
      >
        <img src={appIcon} className="rt-brand-icon" alt="" />
        <b className={stageLayout === "compact" ? "sr-only" : undefined}>
          Vary AI
        </b>
        <small className={stageLayout === "compact" ? "sr-only" : undefined}>
          {address || t("routingPage.listenerUnknown")}
        </small>
        {row && row.candidates.length > 1 && (
          <i className={stageLayout === "compact" ? "sr-only" : undefined}>
            {t(`routingPage.strategy.${row.strategy}`)}
          </i>
        )}
      </div>
      <ol className="rt-accts" aria-label={t("routingPage.candidateSeats")}>
        {!candidates.length && (
          <li className="idle">
            <span>{t(`routingPage.${emptyKey}`)}</span>
            {row?.status != null && row.status >= 400 && row.status !== 499 && (
              <code>{row.status}</code>
            )}
          </li>
        )}
        {candidates.map(({ candidate, trace, index }) => {
          const supplier = upstreams.find(
            (provider) => provider.id === candidate.providerId,
          );
          const chosen = chosenCandidate(trace);
          const active = isAnswering(trace);
          const observedAt =
            (liveObservations ? now : trace.startedAt + trace.durationMs) /
            1000;
          // Current telemetry belongs only to this exact credential and model, never to a historical request.
          const observation = liveObservations
            ? (currentAccounts.find(
                (a) =>
                  candidate.account?.id &&
                  a.id === candidate.account.id &&
                  a.model === candidate.model &&
                  a.providerId === candidate.providerId,
              ) ?? candidate.account)
            : candidate.account;
          const step = [...trace.steps]
            .reverse()
            .find((s) => s.candidate === index);
          const attempt = trace.steps.reduce(
            (last, s, i) =>
              s.kind === "attempt" && s.candidate === index ? i : last,
            -1,
          );
          const wentOn =
            attempt >= 0 &&
            trace.steps.some(
              (s, i) =>
                i > attempt && s.kind === "attempt" && s.candidate !== index,
            );
          const failed =
            ["rejected", "failed", "first_token_timeout"].includes(
              step?.kind ?? "",
            ) ||
            (chosen === index && trace.state === "failed");
          const passed = ["context", "images", "paused"].includes(
            step?.kind ?? "",
          );
          const resting = (observation?.restUntil ?? 0) > observedAt;
          const answering = chosen === index && active;
          const answered = chosen === index && trace.state === "completed";
          const singleKey =
            supplier?.source.kind === "api" &&
            Number(!supplier.transport?.primaryOff && supplier.hasApiKey) +
              (supplier.transport?.keys.filter((key) => key.enabled).length ??
                0) ===
              1;
          const accountKind = singleKey
            ? "provider"
            : (candidate.account?.kind ?? "provider");
          const siblings = candidates
            .map((entry) => entry.candidate)
            .filter(
              (c) =>
                c.providerId === candidate.providerId &&
                c.account?.kind === accountKind,
            );
          const identities = [...new Set(siblings.map((c) => c.account?.id))];
          const number =
            identities.length > 1
              ? identities.indexOf(candidate.account?.id) + 1
              : "";
          const kindLabel =
            accountKind === "provider"
              ? ""
              : t(
                  accountKind === "key"
                    ? "routingPage.keyLabel"
                    : "routingPage.accountLabel",
                  { n: number },
                ).trim();
          const displayName =
            (accountKind !== "provider" && candidate.account?.label) ||
            accountName(candidate);
          const windows =
            observation?.windows.filter(
              (w) => Number.isFinite(w.usedPercent) && w.resetsAt > observedAt,
            ) ?? [];
          const quota = windows.reduce<(typeof windows)[number] | undefined>(
            (fullest, w) =>
              !fullest || w.usedPercent > fullest.usedPercent ? w : fullest,
            undefined,
          );
          const used = quota
            ? Math.max(0, Math.min(100, quota.usedPercent))
            : undefined;
          const remaining = (until: number) => {
            const minutes = Math.max(1, Math.ceil((until - observedAt) / 60));
            return t("routingPage.resetIn", {
              hours: Math.floor(minutes / 60),
              minutes: minutes % 60,
            });
          };
          const status = resting
            ? t("routingPage.seatResting", {
                time: remaining(observation!.restUntil),
              })
            : passed
              ? t(`routingPage.step.${step!.kind}`)
              : failed
                ? t(
                    wentOn
                      ? "routingPage.seatWentOn"
                      : "routingPage.seatFailed",
                  )
                : answering
                  ? t("routingPage.seatAnswering")
                  : answered
                    ? t("routingPage.seatAnswered")
                    : liveObservations &&
                        seat(candidate) &&
                        busy.has(seat(candidate)!)
                      ? t("routingPage.seatAnsweringOther")
                      : quota
                        ? t("routingPage.seatQuota", {
                            percent: Math.round(100 - used!),
                            time: remaining(quota.resetsAt),
                          })
                        : accountKind === "account"
                          ? t("routingPage.accountQuotaUnknown")
                          : kindLabel || t("routingPage.standby");
          return (
            <li
              key={candidateKey(candidate)}
              style={
                {
                  "--rt-agent": hues[trace.agent] ?? "var(--rt-accent)",
                } as CSSProperties
              }
              className={`${answering || answered ? "on" : ""} ${answering ? "now" : ""} ${failed || resting ? "rest" : ""} ${passed ? "left" : ""} ${used != null && used >= 90 ? "low" : ""}`}
              title={`${accountText(displayName)} · ${candidate.model}${candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""} · ${status}`}
              tabIndex={0}
            >
              <span className="dot" />
              <b>
                <span className="rt-seat-icon">
                  <UpstreamIcon
                    icon={supplier ? upstreamIcon(supplier) : undefined}
                    name={accountText(supplier?.name ?? candidate.providerName)}
                    size={16}
                  />
                </span>
                <span className="who">
                  <AccountLabel>{displayName}</AccountLabel>
                  {!candidate.account?.label && number
                    ? ` \u00b7 ${kindLabel}`
                    : ""}
                </span>
                <code className="mdl" title={candidate.model}>
                  {candidate.model}
                  {candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""}
                </code>
              </b>
              <em className="rt-seat-status">
                {step?.status != null &&
                  step.status >= 400 &&
                  step.status !== 499 && <code>{step.status}</code>}
                {status}
              </em>
              {used != null && (
                <div
                  className="rt-seat-bar"
                  role="progressbar"
                  aria-label={t("routingPage.accountQuotaLeft")}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={100 - used}
                >
                  <i style={{ width: `${100 - used}%` }} />
                </div>
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}
