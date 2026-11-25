// Adapted from the reference routing stage (MIT), without credential-derived labels.
// See third_party/routing-reference/LICENSE.
import { Bot, ChevronDown, Share2 } from "lucide-react";
import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
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
import { attemptCandidateIndex } from "./routingAttempts";
import { formatRoutingDuration } from "@/lib/routingDuration";
import { routingSourceKey, useRoutingFlights } from "./useRoutingFlights";
import type {
  RoutingAccount,
  RoutingCandidate,
  RoutingTrace,
} from "@/lib/api/routingTrace";

function live(row?: RoutingTrace) {
  return row?.state === "routing" || row?.state === "streaming";
}
const AGENT_PALETTE = [
  "#d97757",
  "#bd8638",
  "#a59234",
  "#849e4a",
  "#5aa66c",
  "#25a98f",
  "#00a6b0",
  "#2e9fc9",
  "#5e95d7",
  "#858ad7",
  "#a47fca",
  "#bb77b1",
];
const AGENT_HUE_INDEX: Record<string, number> = {
  claude: 0,
  "claude-desktop": 0,
  codex: 9,
  gemini: 7,
  "gemini-cli": 7,
  copilot: 10,
  cursor: 5,
  opencode: 2,
  crush: 11,
  goose: 3,
  pi: 4,
  omp: 11,
  omo: 10,
  dsh: 6,
  qoder: 8,
  qwen: 6,
  trae: 1,
  windsurf: 5,
};
function assignAgentHues(
  agents: Array<string | undefined>,
): Map<string, string> {
  const assigned = new Map<string, string>();
  const used = new Set<string>();
  for (const rawId of agents) {
    if (!rawId || assigned.has(rawId)) continue;
    let start = AGENT_HUE_INDEX[rawId];
    if (start == null) {
      let hash = 0;
      for (const ch of rawId) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
      start = hash % AGENT_PALETTE.length;
    }
    let color = AGENT_PALETTE[start];
    for (let offset = 0; offset < AGENT_PALETTE.length; offset++) {
      const candidate = AGENT_PALETTE[(start + offset) % AGENT_PALETTE.length];
      if (!used.has(candidate)) {
        color = candidate;
        break;
      }
    }
    used.add(color);
    assigned.set(rawId, color);
  }
  return assigned;
}
const seat = (c: RoutingCandidate) =>
  c.account?.id
    ? `${c.account.id}\0${c.model}\0${c.fixedEffort ?? ""}`
    : undefined;
const candidateKey = (trace: RoutingTrace, candidate: RoutingCandidate) =>
  JSON.stringify([
    trace.agent,
    trace.model,
    candidate.providerId,
    candidate.account?.kind,
    candidate.account?.id,
    candidate.model,
    candidate.fixedEffort,
  ]);
function chosenCandidate(trace?: RoutingTrace) {
  if (!trace) return undefined;
  for (let stepIndex = trace.steps.length - 1; stepIndex >= 0; stepIndex--) {
    const index = attemptCandidateIndex(trace, trace.steps[stepIndex]);
    if (index != null) return index;
  }
  return undefined;
}
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
  modelLabel,
  address,
  currentAccounts = [],
  upstreams = [],
  concurrent = [],
  sources,
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
  modelLabel?: (row: RoutingTrace) => string;
  address?: string;
  currentAccounts?: RoutingAccount[];
  upstreams?: RoutingUpstream[];
  concurrent?: RoutingTrace[];
  sources?: RoutingTrace[];
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
  const sourceIds = sources && new Set(sources.map(routingSourceKey));
  const visibleFlights = sourceIds
    ? flights.filter((trace) => sourceIds.has(routingSourceKey(trace)))
    : flights;
  const traces = [
    ...new Map(
      [...visibleFlights, ...(sources ?? (row ? [row] : []))].map((trace) => [
        trace.id,
        trace,
      ]),
    ).values(),
  ];
  const gatewayWorking = animate && !replay && traces.some(live);
  const sourceRows = sources
    ? [...sources].sort((a, b) => a.agent.localeCompare(b.agent))
    : [
        ...new Map(
          traces.map((trace) => [routingSourceKey(trace), trace]),
        ).values(),
      ];
  const sourceCards = sourceRows.length ? sourceRows : [undefined];
  const sourceKeys = sourceCards.map(
    (trace) => trace && routingSourceKey(trace),
  );
  const agentHues = assignAgentHues([
    row?.agent,
    ...traces.map((trace) => trace.agent),
    ...sourceRows.map((trace) => trace.agent),
  ]);
  const agentHue = (agent?: string) =>
    (agent ? agentHues.get(agent) : undefined) ?? "var(--rt-accent)";
  const currentSource = row && routingSourceKey(row);
  const seats = new Map<
    string,
    { candidate: RoutingCandidate; trace: RoutingTrace; index: number }
  >();
  for (const trace of traces) {
    for (let index = 0; index < trace.candidates.length; index++) {
      const candidate = trace.candidates[index];
      const key = candidateKey(trace, candidate);
      const previous = seats.get(key);
      if (
        !previous ||
        (isAnswering(trace) &&
          chosenCandidate(trace) === index &&
          (!isAnswering(previous.trace) ||
            chosenCandidate(previous.trace) !== previous.index)) ||
        (!live(previous.trace) &&
          (live(trace) || trace.startedAt > previous.trace.startedAt))
      )
        seats.set(key, { candidate, trace, index });
    }
  }
  const candidates = [...seats.values()];
  const routeKeyOf = (trace: RoutingTrace) =>
    trace.groupName?.trim() || modelLabel?.(trace) || trace.model;
  const isRoutingGroup = (trace: RoutingTrace) => {
    const name = trace.groupName?.trim();
    return Boolean(
      name && name !== "Direct model" && trace.strategy !== "direct",
    );
  };
  const routeGroups: Array<{
    key: string;
    label: string;
    modelRef: string;
    isGroup: boolean;
    strategy: string;
    agents: string[];
    items: typeof candidates;
  }> = [];
  const routeGroupMap = new Map<string, (typeof routeGroups)[number]>();
  for (const entry of candidates) {
    const key = routeKeyOf(entry.trace);
    let group = routeGroupMap.get(key);
    if (!group) {
      group = {
        key,
        label: key,
        modelRef: entry.trace.model,
        isGroup: isRoutingGroup(entry.trace),
        strategy: entry.trace.strategy,
        agents: [],
        items: [],
      };
      routeGroupMap.set(key, group);
      routeGroups.push(group);
    }
    const agentId = APP_IDS.find((id) => id === entry.trace.agent);
    const agentDisplay = agentId
      ? agentId === "claude"
        ? t("apps.claudeCode")
        : t(`apps.${agentId}`)
      : entry.trace.agent;
    if (agentDisplay && !group.agents.includes(agentDisplay)) {
      group.agents.push(agentDisplay);
    }
    group.items.push(entry);
  }
  const hasGroupHeaders = (group: (typeof routeGroups)[number]) =>
    stageLayout !== "compact" && (group.isGroup || routeGroups.length > 1);
  const [collapsedSubs, setCollapsedSubs] = useState<Record<string, boolean>>(
    {},
  );
  const visibleCandidates = routeGroups.flatMap((group) =>
    hasGroupHeaders(group) && collapsedSubs[group.key] ? [] : group.items,
  );
  const providersByName = new Map<string, Set<string>>();
  for (const provider of [
    ...upstreams.map(({ id, name }) => ({ id, name })),
    ...candidates.map(({ candidate }) => ({
      id: candidate.sourceProviderId ?? candidate.providerId,
      name: accountName(candidate),
    })),
  ]) {
    const name = provider.name.trim().toLocaleLowerCase();
    const ids = providersByName.get(name) ?? new Set<string>();
    ids.add(provider.id);
    providersByName.set(name, ids);
  }
  const candidateIndex = new Map(
    visibleCandidates.map(({ candidate, trace }, index) => [
      candidateKey(trace, candidate),
      index,
    ]),
  );
  const [paths, setPaths] = useState<{
    sources: string[];
    subs: { key: string; d: string }[];
    targets: string[];
  }>({
    sources: [],
    subs: [],
    targets: [],
  });
  const [narrow, setNarrow] = useState(false);
  const layoutKey = JSON.stringify([
    sourceKeys,
    visibleCandidates.map(({ candidate, trace }) =>
      candidateKey(trace, candidate),
    ),
    routeGroups.map((group) => [
      group.key,
      hasGroupHeaders(group),
      !!collapsedSubs[group.key],
    ]),
  ]);
  const busy = new Set(
    concurrent
      .filter((r) => r.id !== row?.id && live(r))
      .flatMap((r) => {
        const index = chosenCandidate(r);
        const last = [...r.steps]
          .reverse()
          .find((step) => step.candidate === index);
        if (
          index == null ||
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
      const subElements = [
        ...node.querySelectorAll<HTMLElement>(".rt-accts li.rt-sub"),
      ];
      const subRects = new Map<string, DOMRect>();
      for (const el of subElements) {
        const key = el.getAttribute("data-sub-key");
        if (key) subRects.set(key, el.getBoundingClientRect());
      }
      const candidateElements = [
        ...node.querySelectorAll<HTMLElement>(
          ".rt-accts li:not(.idle):not(.rt-sub)",
        ),
      ];
      if (!sources.length || !hub) return;
      const wire = (a: DOMRect, b: DOMRect, source = false) => {
        if (b.left < a.right) {
          const x1 = a.left + a.width / 2 - box.left,
            y1 = a.bottom - box.top;
          const x2 = b.left - box.left,
            y2 = b.top + b.height / 2 - box.top;
          return `M${x1},${y1} C${x1},${y1 + 12} ${x2 - 12},${y1 + 12} ${x2 - 12},${y2 - 8} Q${x2 - 12},${y2} ${x2},${y2}`;
        }
        const compact = stageLayout === "compact";
        const x1 = a.right - box.left + (compact ? (source ? 3 : 2) : 0),
          y1 = a.top + a.height / 2 - box.top;
        const x2 = b.left - box.left - (compact && source ? 2 : 0),
          y2 = b.top + b.height / 2 - box.top;
        const middle = (x1 + x2) / 2;
        return `M${x1},${y1} C${middle},${y1} ${middle},${y2} ${x2},${y2}`;
      };
      const fromSub = (sub: DOMRect, target: DOMRect) => {
        const subLeft = sub.left - box.left;
        const x = subLeft + 9;
        const y1 = sub.top + sub.height / 2 - box.top;
        const x2 = target.left - box.left;
        const y2 = target.top + target.height / 2 - box.top;
        return `M${subLeft} ${y1} Q${x} ${y1} ${x} ${y1 + 9} L${x} ${y2 - 9} Q${x} ${y2} ${x2} ${y2}`;
      };
      setPaths({
        sources: sources.map((source) =>
          sourceList &&
          (source.bottom <= sourceList.top || source.top >= sourceList.bottom)
            ? ""
            : wire(source, hub, true),
        ),
        subs: subElements.map((el) => {
          const key = el.getAttribute("data-sub-key") ?? "";
          const rect = el.getBoundingClientRect();
          const d =
            list && (rect.bottom <= list.top || rect.top >= list.bottom)
              ? ""
              : wire(hub, rect);
          return { key, d };
        }),
        targets: candidateElements.map((el) => {
          const candidate = el.getBoundingClientRect();
          if (
            list &&
            (candidate.bottom <= list.top || candidate.top >= list.bottom)
          )
            return "";
          const parentKey = el.getAttribute("data-parent-sub");
          const parentRect = parentKey ? subRects.get(parentKey) : undefined;
          return parentRect
            ? fromSub(parentRect, candidate)
            : wire(hub, candidate);
        }),
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
  const hubDescription = `Vary AI · ${address || t("routingPage.listenerUnknown")}`;
  const packetPaths = (trace: RoutingTrace) => [
    paths.sources[sourceKeys.indexOf(routingSourceKey(trace))] ?? "",
    ...trace.candidates.map(
      (candidate) =>
        paths.targets[
          candidateIndex.get(candidateKey(trace, candidate)) ?? -1
        ] ?? "",
    ),
  ];
  return (
    <div
      className={`rt-stage${narrow ? " narrow" : ""}${stageLayout === "compact" ? " compact" : ""}`}
      ref={stage}
      style={
        {
          "--rt-agent": agentHue(row?.agent),
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
                  return sourceKeys[index] === routingSourceKey(trace);
                const chosen = chosenCandidate(trace);
                const target =
                  chosen == null ? undefined : trace.candidates[chosen];
                return (
                  target &&
                  candidateIndex.get(candidateKey(trace, target)) ===
                    index - paths.sources.length
                );
              });
              if (matching.some((trace) => isAnswering(trace)) && animate)
                return "live";
              if (
                matching.some(
                  (trace) => trace.state === "completed" || isAnswering(trace),
                )
              )
                return "lit";
              if (index >= paths.sources.length) return "rest";
              return "";
            })()}
          />
        ))}
        {paths.subs.map(({ key, d }) => {
          const groupTraces = traces.filter(
            (trace) => routeKeyOf(trace) === key,
          );
          const subClass =
            groupTraces.some((trace) => isAnswering(trace)) && animate
              ? "live"
              : groupTraces.some(
                    (trace) =>
                      trace.state === "completed" || isAnswering(trace),
                  )
                ? "lit"
                : "";
          return <path key={`sub:${key}`} d={d} className={subClass} />;
        })}
      </svg>
      <RoutingPackets
        compact={stageLayout === "compact"}
        row={row}
        paths={row ? packetPaths(row) : [""]}
        flights={
          animate && !replay
            ? visibleFlights.map((trace) => ({
                row: trace,
                color: agentHue(trace.agent),
                paths: packetPaths(trace),
              }))
            : undefined
        }
        onFinished={finish}
        active={animate}
        replay={replay}
        speed={speed}
      />
      <div className={`rt-srcs${sourceCards.length > 1 ? " many" : ""}`}>
        {sourceCards.map((sourceTrace, index) => {
          const source = sourceTrace?.agent;
          const key = sourceKeys[index];
          const agent = APP_IDS.find((id) => id === source);
          const name = agent
            ? agent === "claude"
              ? t("apps.claudeCode")
              : t(`apps.${agent}`)
            : source || t("routingPage.agent");
          const selectedSource = key === currentSource;
          const sourceModel =
            sourceTrace && modelLabel
              ? modelLabel(sourceTrace)
              : selectedSource
                ? requestedModel
                : sourceTrace?.model;
          return (
            <div
              key={key ?? "idle"}
              className={`rt-node rt-src${sourceTrace ? " has-agent" : ""}${traces.some((trace) => routingSourceKey(trace) === key && isAnswering(trace)) ? " now" : ""}`}
              style={
                {
                  "--rt-agent": agentHue(source),
                } as CSSProperties
              }
              title={
                modelLabel
                  ? [name, sourceModel].filter(Boolean).join(" · ")
                  : sourceModel
              }
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
        {routeGroups.map((group) => {
          const showSub = hasGroupHeaders(group);
          const isOpen = !collapsedSubs[group.key];
          const groupActive = group.items.some(
            ({ trace }) => isAnswering(trace) || trace.state === "completed",
          );
          return (
            <Fragment key={group.key}>
              {showSub && (
                <li
                  className={`rt-sub${isOpen ? " open" : ""}${groupActive ? " on" : ""}`}
                  data-sub-key={group.key}
                  role="button"
                  tabIndex={0}
                  aria-expanded={isOpen}
                  onClick={() =>
                    setCollapsedSubs((prev) => ({
                      ...prev,
                      [group.key]: !prev[group.key],
                    }))
                  }
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setCollapsedSubs((prev) => ({
                        ...prev,
                        [group.key]: !prev[group.key],
                      }));
                    }
                  }}
                >
                  <Share2 size={14} aria-hidden="true" />
                  <span className="n">{group.label}</span>
                  {group.modelRef &&
                    group.modelRef !== group.label &&
                    !group.modelRef.startsWith("vary-auto/") && (
                      <code className="mdl">{group.modelRef}</code>
                    )}
                  <span className="chev" aria-hidden="true">
                    <ChevronDown size={13} />
                  </span>
                </li>
              )}
              {(!showSub || isOpen) &&
                group.items.map(({ candidate, trace, index }) => {
                  const supplier = upstreams.find(
                    (provider) =>
                      provider.id ===
                      (candidate.sourceProviderId ?? candidate.providerId),
                  );
                  const chosen = chosenCandidate(trace);
                  const active = isAnswering(trace);
                  const observedAt =
                    (liveObservations
                      ? now
                      : trace.startedAt + trace.durationMs) / 1000;
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
                        i > attempt &&
                        s.candidate !== index &&
                        attemptCandidateIndex(trace, s) != null,
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
                  const answered =
                    chosen === index && trace.state === "completed";
                  const singleKey =
                    supplier?.source.kind === "api" &&
                    Number(
                      !supplier.transport?.primaryOff && supplier.hasApiKey,
                    ) +
                      (supplier.transport?.keys.filter((key) => key.enabled)
                        .length ?? 0) ===
                      1;
                  const accountKind = singleKey
                    ? "provider"
                    : (candidate.account?.kind ?? "provider");
                  const providerName = accountName(candidate);
                  const duplicateName =
                    (providersByName.get(
                      providerName.trim().toLocaleLowerCase(),
                    )?.size ?? 0) > 1;
                  const displayName =
                    (accountKind !== "provider" && candidate.account?.label) ||
                    (duplicateName
                      ? `${providerName} · ${candidate.sourceProviderId ?? candidate.providerId}`
                      : providerName);
                  const plan =
                    accountKind === "account" ? observation?.plan : undefined;
                  const windows =
                    observation?.windows.filter(
                      (w) =>
                        Number.isFinite(w.usedPercent) &&
                        w.resetsAt > observedAt,
                    ) ?? [];
                  const quota = windows.reduce<
                    (typeof windows)[number] | undefined
                  >(
                    (fullest, w) =>
                      !fullest || w.usedPercent > fullest.usedPercent
                        ? w
                        : fullest,
                    undefined,
                  );
                  const used = quota
                    ? Math.max(0, Math.min(100, quota.usedPercent))
                    : undefined;
                  const remaining = (until: number) => {
                    return t("routingPage.resetIn", {
                      time: formatRoutingDuration(
                        (until - observedAt) * 1000,
                        t,
                      ),
                    });
                  };
                  const baseStatus = resting
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
                                  : t(
                                      accountKind === "key"
                                        ? "routingPage.apiKey"
                                        : "routingPage.oneKey",
                                    );
                  // Keep the renewal visible when another state takes the status line.
                  const replying = answering && !resting && !passed && !failed;
                  const showResponse =
                    replying || (answered && !resting && !passed && !failed);
                  const sessionTitle = showResponse
                    ? trace.sessionTitle?.trim()
                    : undefined;
                  const sessionLabel = sessionTitle
                    ? accountText(sessionTitle)
                    : showResponse &&
                        trace.session &&
                        /^[a-f0-9]{64}$/.test(trace.session)
                      ? t("routingPage.sessionReference", {
                          id: trace.session.slice(0, 8),
                        })
                      : undefined;
                  const rate = showResponse
                    ? trace.outputTokensPerSecond
                    : undefined;
                  const rateLabel =
                    rate != null && Number.isFinite(rate) && rate > 0
                      ? t("routingPage.outputRate", {
                          value: rate,
                          formatParams: { value: { maximumFractionDigits: 1 } },
                        })
                      : undefined;
                  const renewal =
                    quota && !resting ? remaining(quota.resetsAt) : undefined;
                  const status = [
                    baseStatus,
                    sourceRows.length > 1 ? modelLabel?.(trace) : undefined,
                    sessionLabel,
                    rateLabel,
                    renewal && !baseStatus.includes(renewal)
                      ? renewal
                      : undefined,
                  ]
                    .filter(Boolean)
                    .join(" · ");
                  return (
                    <li
                      key={candidateKey(trace, candidate)}
                      data-parent-sub={showSub ? group.key : undefined}
                      style={
                        {
                          "--rt-agent": agentHue(trace.agent),
                          "--depth": showSub ? 1 : 0,
                        } as CSSProperties
                      }
                      className={`${answering || answered ? "on" : ""} ${answering ? "now" : ""} ${replying && animate && !replay ? "replying" : ""} ${failed || resting ? "rest" : ""} ${passed ? "left" : ""} ${used != null && used >= 90 ? "low" : ""}`}
                      title={`${accountText(displayName)} · ${candidate.model}${candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""} · ${status}`}
                      tabIndex={0}
                    >
                      {stageLayout !== "compact" && (
                        <span className="rt-seat-dot" aria-hidden="true" />
                      )}
                      <b>
                        <span className="rt-seat-icon">
                          <UpstreamIcon
                            icon={supplier ? upstreamIcon(supplier) : undefined}
                            name={accountText(
                              supplier?.name ?? candidate.providerName,
                            )}
                            size={16}
                          />
                        </span>
                        <span className="who">
                          <AccountLabel>{displayName}</AccountLabel>
                        </span>
                        {plan && <span className="rt-seat-plan">{plan}</span>}
                        <code className="mdl" title={candidate.model}>
                          {candidate.model}
                          {candidate.fixedEffort
                            ? ` :${candidate.fixedEffort}`
                            : ""}
                        </code>
                      </b>
                      <em
                        className={`rt-seat-status${sessionLabel || rateLabel ? " has-response" : ""}`}
                      >
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
            </Fragment>
          );
        })}
      </ol>
    </div>
  );
}
