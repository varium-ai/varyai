import {
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useQuery } from "@tanstack/react-query";
import { Box, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { fmtUsd, getLocaleFromLanguage } from "@/components/usage/format";
import { getUsageSourceLabel } from "@/components/usage/source";
import {
  trayUsageApi,
  trayUsageKey,
  type TrayUsageRow,
} from "@/lib/api/trayUsage";
import { useUsageModelLabels } from "@/lib/query/usage";
import {
  activityDates,
  activityLevels,
  activityWeeks,
  allProviders,
  axisTop,
  groupUsage,
  localDateKey,
  localSessions,
  metrics,
  metricValue,
  periods,
  providerKey,
  seriesKey,
  splits,
  sumUsage,
  trendBuckets,
  type UsageMetric,
} from "./trayUsageData";
import "./tray-usage.css";

const colors = [
  "#5144e8",
  "#0ca6da",
  "#169988",
  "#da8e27",
  "#bd60b4",
  "#6d88be",
];
const noRows: TrayUsageRow[] = [];

function useChoice<Choice extends string>(
  key: string,
  options: readonly Choice[],
) {
  const storageKey = `vary:tray-usage-${key}`;
  const [value, setValue] = useState(
    () =>
      options.find((option) => option === localStorage.getItem(storageKey)) ??
      options[0],
  );
  return [
    value,
    (next: Choice) => {
      localStorage.setItem(storageKey, next);
      setValue(next);
    },
  ] as const;
}

function Segments<Choice extends string>({
  label,
  choices,
  value,
  onChange,
}: {
  label: string;
  choices: readonly Choice[];
  value: Choice;
  onChange: (value: Choice) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="tray-use-segments" role="group" aria-label={label}>
      {choices.map((choice) => (
        <button
          type="button"
          key={choice}
          aria-pressed={choice === value}
          onClick={() => onChange(choice)}
        >
          {t(`trayUsage.${choice}`)}
        </button>
      ))}
    </div>
  );
}

function formatCompact(value: number, locale: string) {
  const unit = [
    [1e12, "T"],
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ] as const;
  const scale = unit.find(([threshold]) => value >= threshold);
  const precision =
    value >= 1e9 ? 2 : value >= 1e7 || (value >= 1e5 && value < 1e6) ? 0 : 1;
  return (
    new Intl.NumberFormat(locale, { maximumFractionDigits: precision }).format(
      scale ? value / scale[0] : value,
    ) + (scale?.[1] ?? "")
  );
}

const heatMetrics = ["tokens", "requests", "cost"] as const;
const heatWeekdays = [0, 1, 2, 3, 4, 5, 6];
const dayLabel = (day: string) => new Date(`${day}T12:00:00`);

function Activity({ active, provider }: { active: boolean; provider: string }) {
  const { t, i18n } = useTranslation();
  const [metric, setMetric] = useChoice<UsageMetric>(
    "heat-metric",
    heatMetrics,
  );
  const titleId = useId();
  const card = useRef<HTMLElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const [focusDay, setFocusDay] = useState<string>();
  const [tip, setTip] = useState<{ index: number; x: number; y: number }>();
  const [scrollbar, setScrollbar] = useState({ position: 1000, width: 50 });
  const { data } = useQuery({
    queryKey: [...trayUsageKey, "activity"],
    queryFn: () => trayUsageApi.read("activity"),
    enabled: active,
    staleTime: 60000,
    refetchInterval: active ? 60000 : false,
  });
  const grouped = useMemo(
    () =>
      groupUsage(
        (data ?? noRows).filter(
          (row) => provider === allProviders || providerKey(row) === provider,
        ),
        (row) => row.bucket,
      ),
    [data, provider],
  );
  const today = localDateKey(new Date());
  const dates = useMemo(() => activityDates(dayLabel(today)), [today]);
  const daily = useMemo(
    () => dates.map((day) => sumUsage(grouped.get(day) ?? noRows)),
    [dates, grouped],
  );
  const hasActivity = daily.some(({ requests }) => requests > 0);
  const measureScroll = (viewport: HTMLDivElement) => {
    const range = viewport.scrollWidth - viewport.clientWidth;
    if (range <= 0) return;
    const position = Math.round((1000 * viewport.scrollLeft) / range);
    const width = (100 * viewport.clientWidth) / viewport.scrollWidth;
    setScrollbar((previous) =>
      previous.position === position && previous.width === width
        ? previous
        : { position, width },
    );
  };
  useLayoutEffect(() => {
    const viewport = scroll.current;
    if (!hasActivity || !active || !viewport) return;
    viewport.scrollLeft = viewport.scrollWidth;
    measureScroll(viewport);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => measureScroll(viewport));
    observer.observe(viewport);
    return () => observer.disconnect();
  }, [hasActivity, active]);
  const locale = getLocaleFromLanguage(i18n.language);
  const days = useMemo(() => {
    const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "full" });
    const levels = activityLevels(
      daily.map((total) => metricValue(total, metric)),
    );
    const order = [metric, ...heatMetrics.filter((key) => key !== metric)];
    return daily.map((totals, index) => {
      const values = totals.requests
        ? {
            tokens: `${formatCompact(metricValue(totals, "tokens"), locale)} ${t("trayUsage.tokens")}`,
            requests: t("trayUsage.callCount", { count: totals.requests }),
            cost:
              totals.cost || !totals.unpriced ? fmtUsd(totals.cost, 4) : "—",
          }
        : null;
      return {
        day: dates[index],
        label: dateFormat.format(dayLabel(dates[index])),
        detail: values
          ? order.map((key) => values[key]).join(" · ")
          : t("trayUsage.activityNoRequests"),
        level: levels[index],
      };
    });
  }, [daily, dates, locale, metric, t]);
  if (!hasActivity) return null;
  const monthFormat = new Intl.DateTimeFormat(locale, { month: "short" });
  const weekdayFormat = new Intl.DateTimeFormat(locale, { weekday: "short" });
  const months: { week: number; name: string }[] = [];
  for (let week = 0; week < activityWeeks; week++) {
    const date = dayLabel(dates[week * 7]);
    if (week === 0 || date.getDate() <= 7) {
      months.push({ week, name: monthFormat.format(date) });
    }
  }
  const focusIndex = dates.indexOf(focusDay ?? "");
  const tabIndex = focusIndex < 0 ? dates.length - 1 : focusIndex;
  const tipPosition = (index: number, node: HTMLElement) => {
    const bounds = card.current?.getBoundingClientRect();
    if (!bounds) return;
    const cell = node.getBoundingClientRect();
    return {
      index,
      x: cell.left + cell.width / 2 - bounds.left,
      y: cell.top - bounds.top - 5,
    };
  };
  return (
    <section
      ref={card}
      className="tray-use-card tray-use-activity"
      aria-labelledby={titleId}
    >
      <div className="tray-use-activity-head">
        <div>
          <h2 id={titleId}>{t("trayUsage.activity")}</h2>
          <p>{t("trayUsage.activityRange")}</p>
        </div>
        <Segments
          label={t("trayUsage.activityMetric")}
          choices={heatMetrics}
          value={metric}
          onChange={setMetric}
        />
      </div>
      <div
        ref={scroll}
        className="tray-use-heat-scroll"
        onMouseLeave={() => setTip(undefined)}
        onScroll={(event) => {
          measureScroll(event.currentTarget);
          const focused = document.activeElement as HTMLElement | null;
          if (focused?.dataset.date && event.currentTarget.contains(focused)) {
            const position = tipPosition(
              dates.indexOf(focused.dataset.date),
              focused,
            );
            setTip((current) =>
              current && current.index === position?.index
                ? position
                : undefined,
            );
          } else setTip(undefined);
        }}
      >
        <div className="tray-use-heat">
          {months.map(({ week, name }, index) => (
            <span
              key={week}
              className="tray-use-heat-month"
              style={{
                gridColumn: `${week + 2} / ${(months[index + 1]?.week ?? activityWeeks) + 2}`,
              }}
            >
              {name}
            </span>
          ))}
          {heatWeekdays.map((weekday) => (
            <span
              key={weekday}
              className="tray-use-heat-weekday"
              style={{ gridRow: weekday + 2 }}
              aria-hidden={weekday % 2 !== 0}
            >
              {weekday % 2 === 0 &&
                weekdayFormat.format(dayLabel(dates[weekday]))}
            </span>
          ))}
          {days.map(({ day, label, detail, level }, index) => (
            <button
              key={day}
              type="button"
              className="tray-use-heat-cell"
              data-date={day}
              data-level={level}
              style={{
                gridColumn: Math.floor(index / 7) + 2,
                gridRow: (index % 7) + 2,
              }}
              tabIndex={index === tabIndex ? 0 : -1}
              aria-label={`${label} · ${detail}`}
              onMouseEnter={(event) =>
                setTip(tipPosition(index, event.currentTarget))
              }
              onFocus={(event) => {
                setFocusDay(day);
                setTip(tipPosition(index, event.currentTarget));
              }}
              onBlur={() => setTip(undefined)}
              onKeyDown={(event) => {
                if (event.key === "Escape" && tip) {
                  event.preventDefault();
                  setTip(undefined);
                  return;
                }
                const steps: Record<string, number> = {
                  ArrowLeft: -7,
                  ArrowRight: 7,
                  ArrowUp: -1,
                  ArrowDown: 1,
                  Home: -index,
                  End: dates.length - 1 - index,
                };
                const step = steps[event.key];
                if (typeof step !== "number") return;
                event.preventDefault();
                const next =
                  dates[Math.min(dates.length - 1, Math.max(0, index + step))];
                scroll.current
                  ?.querySelector<HTMLButtonElement>(`[data-date="${next}"]`)
                  ?.focus();
              }}
            />
          ))}
        </div>
      </div>
      <input
        className="tray-use-heat-scrollbar"
        type="range"
        min={0}
        max={1000}
        value={scrollbar.position}
        aria-label={t("trayUsage.activityScroll")}
        style={{ "--heat-thumb": `${scrollbar.width}%` } as CSSProperties}
        onChange={(event) => {
          const viewport = scroll.current;
          if (!viewport) return;
          viewport.scrollLeft =
            ((viewport.scrollWidth - viewport.clientWidth) *
              event.currentTarget.valueAsNumber) /
            1000;
          measureScroll(viewport);
        }}
      />
      <div className="tray-use-heat-legend">
        <span>{t("trayUsage.activityLess")}</span>
        {[0, 1, 2, 3, 4].map((level) => (
          <i key={level} data-level={level} aria-hidden="true" />
        ))}
        <span>{t("trayUsage.activityMore")}</span>
      </div>
      {active && tip && dates[tip.index] && (
        <div
          className="tray-use-heat-tip"
          role="tooltip"
          style={
            {
              "--heat-tip-x": `${tip.x}px`,
              "--heat-tip-y": `${tip.y}px`,
            } as CSSProperties
          }
        >
          <strong>{days[tip.index].label}</strong>
          <span>{days[tip.index].detail}</span>
        </div>
      )}
    </section>
  );
}

export function TrayUsagePanel({
  active,
  onOpen,
}: {
  active: boolean;
  onOpen: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [period, setPeriod] = useChoice("period", periods);
  const [metric, setMetric] = useChoice("metric", metrics);
  const [splitChoice, setSplit] = useChoice("split", splits);
  const [providerChoice, setProvider] = useState(allProviders);
  const [day, setDay] = useState<string>();
  const [tipKey, setTipKey] = useState<string>();
  const tipId = useId();
  const query = useQuery({
    queryKey: [...trayUsageKey, period],
    queryFn: () => trayUsageApi.read(period),
    enabled: active,
    staleTime: 2000,
    refetchInterval: active ? 10000 : false,
  });
  const rows = query.data ?? noRows;
  const providerNames = useMemo(() => {
    const names = new Map<string, string>();
    for (const row of rows) {
      const key = providerKey(row);
      names.set(
        key,
        key === localSessions
          ? t("trayUsage.sessions")
          : getUsageSourceLabel(row, t),
      );
    }
    return new Map(
      [...names].sort(([, left], [, right]) =>
        left.localeCompare(right, i18n.language),
      ),
    );
  }, [rows, t, i18n.language]);
  const provider = providerNames.has(providerChoice)
    ? providerChoice
    : allProviders;
  const split = provider === allProviders ? splitChoice : "model";
  const filtered = useMemo(
    () =>
      provider === allProviders
        ? rows
        : rows.filter((row) => providerKey(row) === provider),
    [rows, provider],
  );
  const tokens = (value: number) =>
    formatCompact(value, getLocaleFromLanguage(i18n.language));
  const format = (value: number) =>
    metric === "cost"
      ? fmtUsd(value, value > 0 && value < 0.01 ? 4 : 2)
      : tokens(value);
  const pct = (fraction: number) =>
    new Intl.NumberFormat(i18n.language, {
      style: "percent",
      maximumFractionDigits: 0,
    }).format(fraction);
  const bucket = trendBuckets(filtered, period);
  const chosenDay =
    bucket.selectable && day && bucket.keys.includes(day) ? day : undefined;
  const visible = chosenDay
    ? (bucket.grouped.get(chosenDay) ?? noRows)
    : filtered;
  const total = sumUsage(visible);
  const prompt = total.input + total.reads + total.writes;
  const keyOf = seriesKey(split);
  const rank = (source: TrayUsageRow[]) =>
    [...groupUsage(source, keyOf)]
      .map(([id, hits]) => ({ id, total: sumUsage(hits) }))
      .sort(
        (left, right) =>
          metricValue(right.total, metric) - metricValue(left.total, metric) ||
          left.id.localeCompare(right.id),
      );
  const chartRank = rank(filtered);
  const ranking = chosenDay ? rank(visible) : chartRank;
  const colorOf = new Map(
    chartRank
      .slice(0, colors.length)
      .map(({ id }, index) => [id, colors[index]]),
  );
  const modelNames = useUsageModelLabels(
    split === "model"
      ? [...colorOf.keys(), ...ranking.slice(0, 12).map(({ id }) => id)]
      : [],
    { refetchInterval: false },
  );
  const labelOf = (id: string) =>
    (split === "provider"
      ? providerNames.get(id)
      : (modelNames.data?.find((label) => label.id === id)?.displayName ??
        id)) || t("usage.unknownProvider");
  const columns = bucket.keys.map((key) => {
    const hits = bucket.grouped.get(key) ?? noRows;
    const groups = new Map(groupUsage(hits, keyOf));
    const segments = [...colorOf].map(([id, color]) => ({
      id,
      color,
      value: metricValue(sumUsage(groups.get(id) ?? []), metric),
    }));
    const totals = sumUsage(hits);
    const other = metricValue(
      sumUsage(hits.filter((hit) => !colorOf.has(keyOf(hit)))),
      metric,
    );
    return {
      key,
      requests: totals.requests,
      value: metricValue(totals, metric),
      segments: [
        ...segments,
        { id: null, color: "var(--tu-muted)", value: other },
      ],
    };
  });
  const tipIndex = active
    ? columns.findIndex(({ key, requests }) => key === tipKey && requests > 0)
    : -1;
  const tip = columns[tipIndex];
  const tipTime =
    tip &&
    (period === "today"
      ? `${localDateKey(new Date())} ${tip.key}`
      : tip.key
    ).replaceAll("-", "/");
  const high = Math.max(0, ...columns.map(({ value }) => value));
  const top = high ? axisTop(high) : 1;
  const tickEvery = Math.ceil(columns.length / (period === "week" ? 7 : 6));
  const summary = [
    {
      id: "tokens",
      value: tokens(metricValue(total, "tokens")),
      detail: t("trayUsage.tokenDetail", {
        input: tokens(total.input),
        output: tokens(total.output),
        cache: tokens(total.reads + total.writes),
      }),
    },
    {
      id: "requests",
      value: tokens(total.requests),
      detail: total.failed
        ? t("trayUsage.failCount", { count: total.failed })
        : t("trayUsage.noneFailed"),
    },
    {
      id: "cost",
      value: total.cost > 0 || !total.unpriced ? fmtUsd(total.cost, 2) : "—",
      detail: total.unpriced
        ? t("trayUsage.unpriced", { count: total.unpriced })
        : t("trayUsage.effective"),
    },
    {
      id: "cache",
      value: prompt > 0 ? pct(total.reads / prompt) : "—",
      detail: t("trayUsage.ofPrompt"),
    },
  ];
  return (
    <section className="tray-use" aria-label={t("trayPanel.usage")}>
      <div className="tray-use-toolbar">
        <Segments
          label={t("trayUsage.period")}
          choices={periods}
          value={period}
          onChange={(next) => {
            if (next !== period) {
              setPeriod(next);
              setDay(undefined);
            }
          }}
        />
        <Select value={provider} onValueChange={setProvider}>
          <SelectTrigger
            className="tray-use-provider"
            aria-label={t("trayUsage.provider")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="tray-use-menu" collisionPadding={8}>
            <SelectItem value={allProviders}>
              {t("trayUsage.allProviders")}
            </SelectItem>
            {[...providerNames].map(([id, label]) => (
              <SelectItem key={id} value={id} title={label}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="tray-use-actions">
          <button
            type="button"
            title={t("trayUsage.refresh")}
            aria-label={t("trayUsage.refresh")}
            aria-busy={query.isFetching}
            disabled={query.isFetching}
            onClick={() => void query.refetch()}
          >
            <RefreshCw
              size={14}
              className={
                query.isFetching
                  ? "animate-spin motion-reduce:animate-none"
                  : undefined
              }
            />
          </button>
          <button type="button" onClick={onOpen}>
            {t("trayUsage.open")}
          </button>
        </div>
      </div>
      <p className="tray-use-note">{t("trayUsage.note")}</p>
      {query.isError && (
        <p className="tray-error" role="alert">
          {t("trayUsage.error")}
        </p>
      )}
      {query.isPending ? (
        <div className="tray-use-card tray-use-state" role="status">
          {t("common.loading")}
        </div>
      ) : query.data && total.requests === 0 && !chosenDay ? (
        <div className="tray-use-card tray-use-state">
          <b>
            {t(
              period === "today" && provider === allProviders
                ? "trayUsage.emptyToday"
                : "trayUsage.empty",
            )}
          </b>
          <span>{t("trayUsage.emptyHint")}</span>
        </div>
      ) : (
        query.data && (
          <>
            <dl className="tray-use-card tray-use-totals">
              {summary.map(({ id, value, detail }) => (
                <div key={id}>
                  <dt>{t(`trayUsage.${id}`)}</dt>
                  <dd data-metric={id}>{value}</dd>
                  <dd className="tray-use-detail">{detail}</dd>
                </div>
              ))}
            </dl>
            <div className="tray-use-card tray-use-chart-card">
              <div className="tray-use-chart-head">
                <Segments
                  label={t("trayUsage.metric")}
                  choices={metrics}
                  value={metric}
                  onChange={setMetric}
                />
                {provider === allProviders && (
                  <Segments
                    label={t("trayUsage.split")}
                    choices={splits}
                    value={split}
                    onChange={setSplit}
                  />
                )}
              </div>
              {high > 0 ? (
                <div
                  className="tray-use-chart"
                  role="group"
                  aria-label={t("trayUsage.trend")}
                  onPointerLeave={() => setTipKey(undefined)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape" && tip) {
                      event.preventDefault();
                      setTipKey(undefined);
                    }
                  }}
                >
                  <div className="tray-use-grid" aria-hidden="true">
                    {[4, 3, 2, 1, 0].map((tick) => (
                      <div key={tick}>
                        <span>{format((top * tick) / 4)}</span>
                      </div>
                    ))}
                  </div>
                  <div className="tray-use-columns">
                    {columns.map(
                      ({ key, value, requests, segments }, index) => {
                        const details = `${key} · ${format(value)}`;
                        const activeTip = key === tip?.key;
                        const showTip = () =>
                          setTipKey(requests > 0 ? key : undefined);
                        const tipTarget = {
                          "aria-label": details,
                          "aria-describedby": activeTip ? tipId : undefined,
                          onFocus: showTip,
                          onBlur: () => setTipKey(undefined),
                        };
                        const column = (
                          <span
                            className="tray-use-bar"
                            style={{ height: `${(value * 100) / top}%` }}
                          >
                            {segments
                              .filter((segment) => segment.value > 0)
                              .map((segment) => (
                                <span
                                  key={
                                    segment.id === null
                                      ? "other"
                                      : `series:${segment.id}`
                                  }
                                  style={{
                                    backgroundColor: segment.color,
                                    flexGrow: segment.value,
                                  }}
                                />
                              ))}
                          </span>
                        );
                        return (
                          <div
                            key={key}
                            className="tray-use-slot"
                            data-tip-active={activeTip || undefined}
                            onPointerEnter={showTip}
                            onPointerDown={showTip}
                          >
                            {bucket.selectable && value > 0 ? (
                              <button
                                type="button"
                                {...tipTarget}
                                aria-pressed={chosenDay === key}
                                onClick={() =>
                                  setDay(chosenDay === key ? undefined : key)
                                }
                              >
                                {column}
                              </button>
                            ) : (
                              <span
                                className="tray-use-column"
                                role="img"
                                tabIndex={requests > 0 ? 0 : undefined}
                                {...tipTarget}
                              >
                                {column}
                              </span>
                            )}
                            {index % tickEvery === 0 && (
                              <span
                                className="tray-use-tick"
                                aria-hidden="true"
                              >
                                {key.length === 10 ? key.slice(5) : key}
                              </span>
                            )}
                          </div>
                        );
                      },
                    )}
                  </div>
                  {tip && (
                    <div
                      className="tray-use-tip"
                      id={tipId}
                      role="tooltip"
                      data-side={
                        tipIndex >= columns.length / 2 ? "left" : "right"
                      }
                      style={
                        {
                          "--tu-tip-anchor": `${((tipIndex + 0.5) * 100) / columns.length}%`,
                        } as CSSProperties
                      }
                    >
                      <strong>{tipTime}</strong>
                      <ul>
                        {tip.segments
                          .filter(({ value }) => value > 0)
                          .sort((left, right) => right.value - left.value)
                          .map(({ id, color, value }) => (
                            <li key={id === null ? "other" : `series:${id}`}>
                              <i
                                aria-hidden="true"
                                style={{ backgroundColor: color }}
                              />
                              <span>
                                {id === null
                                  ? t("trayUsage.other")
                                  : labelOf(id)}
                              </span>
                              <span>{format(value)}</span>
                            </li>
                          ))}
                      </ul>
                      <div className="tray-use-tip-total">
                        <span>{t("trayUsage.total")}</span>
                        <span>
                          {format(tip.value)}
                          {metric !== "requests" &&
                            ` · ${t("trayUsage.callCount", { count: tip.requests })}`}
                        </span>
                      </div>
                    </div>
                  )}
                </div>
              ) : (
                <p className="tray-use-state">
                  {t(
                    metric === "cost"
                      ? "trayUsage.noCost"
                      : "trayUsage.noMetric",
                  )}
                </p>
              )}
              {chosenDay && (
                <button
                  className="tray-use-day"
                  type="button"
                  onClick={() => setDay(undefined)}
                  aria-label={t("trayUsage.clearDay")}
                >
                  {chosenDay} ×
                </button>
              )}
              <ul className="tray-use-ranking">
                {ranking.slice(0, 12).map(({ id, total: entry }) => {
                  const value = metricValue(entry, metric);
                  const first = ranking[0]
                    ? metricValue(ranking[0].total, metric)
                    : 0;
                  const share = metricValue(total, metric);
                  const heading = (
                    <>
                      <i aria-hidden="true" />
                      {id === localSessions && (
                        <Box size={12} aria-hidden="true" />
                      )}
                      <span title={labelOf(id)}>{labelOf(id)}</span>
                      <b>{format(value)}</b>
                    </>
                  );
                  const meta = [
                    t("trayUsage.callCount", { count: entry.requests }),
                    entry.cost > 0 || !entry.unpriced
                      ? fmtUsd(entry.cost, 2)
                      : "—",
                    ...(entry.generationMs > 0
                      ? [
                          t("trayUsage.rate", {
                            rate: tokens(
                              (1000 * entry.timedOutput) / entry.generationMs,
                            ),
                            count: entry.timedCount,
                          }),
                        ]
                      : []),
                    ...(entry.firstCount > 0
                      ? [
                          t("trayUsage.firstToken", {
                            time: tokens(
                              entry.firstMs / entry.firstCount / 1000,
                            ),
                          }),
                        ]
                      : []),
                    ...(share > 0 ? [pct(value / share)] : []),
                  ];
                  return (
                    <li
                      key={id}
                      style={
                        {
                          "--tu-series": colorOf.get(id) ?? "var(--tu-muted)",
                        } as CSSProperties
                      }
                    >
                      {split === "provider" ? (
                        <button
                          className="tray-use-rank-title"
                          type="button"
                          onClick={() => setProvider(id)}
                        >
                          {heading}
                        </button>
                      ) : (
                        <div className="tray-use-rank-title">{heading}</div>
                      )}
                      <div className="tray-use-rank-body">
                        <div
                          className="tray-use-rank-bar"
                          style={{
                            width: first > 0 ? `${(100 * value) / first}%` : 0,
                          }}
                        />
                        <p>{meta.join(" · ")}</p>
                      </div>
                    </li>
                  );
                })}
              </ul>
            </div>
          </>
        )
      )}
      <Activity active={active} provider={provider} />
    </section>
  );
}
