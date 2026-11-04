// Adapted from the reference routing stage (MIT), without credential-derived labels.
// See third_party/routing-reference/LICENSE.
import { Bot, Box, KeyRound } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { APP_ICON_NAME, APP_IDS } from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import { AccountLabel, useAccountText } from "@/components/AccountLabel";
import appIcon from "@/assets/icons/app-icon.png";
import { RoutingPackets } from "./RoutingPackets";
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
export function RoutingStage({
  row,
  accountName,
  animate,
  replay,
  speed,
  requestedModel,
  address,
  currentAccounts = [],
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
  concurrent?: RoutingTrace[];
  liveObservations?: boolean;
  layout?: "responsive" | "compact";
  now?: number;
}) {
  const { t } = useTranslation();
  const accountText = useAccountText();
  const stage = useRef<HTMLDivElement>(null);
  const sourceAgents = [row?.agent];
  const [paths, setPaths] = useState<{ sources: string[]; targets: string[] }>({
    sources: [],
    targets: [],
  });
  const [narrow, setNarrow] = useState(false);
  const layoutKey = JSON.stringify([
    sourceAgents,
    row?.candidates.map((candidate) => [
      candidate.providerId,
      candidate.account?.id,
      candidate.model,
      candidate.fixedEffort,
    ]),
  ]);
  const chosen = [...(row?.steps ?? [])]
    .reverse()
    .find((step) => step.kind === "attempt")?.candidate;
  const active =
    live(row) &&
    chosen != null &&
    ![
      "rejected",
      "failed",
      "context",
      "images",
      "first_token_timeout",
      "cancelled",
    ].includes(
      [...(row?.steps ?? [])]
        .reverse()
        .find((step) => step.candidate === chosen)?.kind ?? "",
    );
  const observedAt =
    (liveObservations
      ? now
      : (row?.startedAt ?? now) + (row?.durationMs ?? 0)) / 1000;
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
  const sourceIndex = sourceAgents.indexOf(row?.agent);
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
            className={
              row &&
              (index === sourceIndex || index - paths.sources.length === chosen)
                ? active && animate
                  ? "live"
                  : row.state === "completed" || active
                    ? "lit"
                    : ""
                : ""
            }
          />
        ))}
      </svg>
      <RoutingPackets
        row={row}
        paths={[
          paths.sources[Math.max(0, sourceIndex)] ?? "",
          ...paths.targets.slice(0, row?.candidates.length ?? 0),
        ]}
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
          return (
            <div
              key={source ?? "idle"}
              className={`rt-node rt-src ${selectedSource && active ? "now" : ""}`}
              style={
                {
                  "--rt-agent": hues[source ?? ""] ?? "var(--rt-accent)",
                } as CSSProperties
              }
              title={selectedSource ? requestedModel : undefined}
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
      <div className="rt-node rt-hub" title={hubDescription}>
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
        {!row?.candidates.length && (
          <li className="idle">
            <span>{t(`routingPage.${emptyKey}`)}</span>
            {row?.status != null && row.status >= 400 && (
              <code>{row.status}</code>
            )}
          </li>
        )}
        {row?.candidates.map((candidate, index) => {
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
          const step = [...row.steps]
            .reverse()
            .find((s) => s.candidate === index);
          const attempt = row.steps.reduce(
            (last, s, i) =>
              s.kind === "attempt" && s.candidate === index ? i : last,
            -1,
          );
          const wentOn =
            attempt >= 0 &&
            row.steps.some(
              (s, i) =>
                i > attempt && s.kind === "attempt" && s.candidate !== index,
            );
          const failed =
            ["rejected", "failed", "first_token_timeout"].includes(
              step?.kind ?? "",
            ) ||
            (chosen === index && row.state === "failed");
          const passed = ["context", "images", "paused"].includes(
            step?.kind ?? "",
          );
          const resting = (observation?.restUntil ?? 0) > observedAt;
          const answering = chosen === index && active;
          const answered = chosen === index && row.state === "completed";
          const accountKind = candidate.account?.kind ?? "provider";
          const siblings = row.candidates.filter(
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
              key={`${candidate.account?.id ?? candidate.providerId}/${candidate.model}/${index}`}
              className={`${answering || answered ? "on" : ""} ${answering ? "now" : ""} ${failed || resting ? "rest" : ""} ${passed ? "left" : ""} ${used != null && used >= 90 ? "low" : ""}`}
              title={`${accountText(accountName(candidate))} · ${candidate.model}${candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""} · ${status}`}
              tabIndex={0}
            >
              <span className="dot" />
              <b>
                {accountKind === "account" ? (
                  <ProviderIcon
                    icon="openai"
                    name={t("routingPage.accountLabel", { n: "" })}
                    size={16}
                  />
                ) : accountKind === "key" ? (
                  <KeyRound size={16} aria-hidden="true" />
                ) : (
                  <Box size={16} aria-hidden="true" />
                )}
                <span className="who">
                  <AccountLabel>{accountName(candidate)}</AccountLabel>
                  {number ? ` \u00b7 ${kindLabel}` : ""}
                </span>
                <code className="mdl" title={candidate.model}>
                  {candidate.model}
                  {candidate.fixedEffort ? ` :${candidate.fixedEffort}` : ""}
                </code>
              </b>
              <em className="rt-seat-status">
                {step?.status != null && step.status >= 400 && (
                  <code>{step.status}</code>
                )}
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
