import type { ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { QuotaTier } from "@/types/subscription";
import { QuotaSparkline } from "./QuotaSparkline";

export interface QuotaCardOptions {
  remaining: boolean;
  toggle: () => void;
}

function poolLabel(name: string): string {
  const [family, version] = name.split(/[ &]+/);
  return /^\d/.test(version ?? "") ? `${family} ${version}` : family;
}

export function ProviderQuotaCard({
  name,
  items,
  status,
  fetching,
  loading,
  refresh,
  disabled = false,
  card,
  children,
  sample,
  title,
}: {
  name: string;
  items: Array<{ tier: QuotaTier; label: string }>;
  status: string | null;
  fetching: boolean;
  loading: boolean;
  refresh: () => void;
  disabled?: boolean;
  card: QuotaCardOptions;
  children?: ReactNode;
  sample?: { scope: string; at: number };
  title?: string;
}) {
  const { t, i18n } = useTranslation();
  const resets = items.flatMap(({ tier, label }) =>
    tier.utilization > 0 &&
    tier.resetsAt &&
    Number.isFinite(Date.parse(tier.resetsAt))
      ? [
          {
            label,
            at: new Date(tier.resetsAt),
          },
        ]
      : [],
  );
  const resetText = (compact: boolean) =>
    resets
      .map(({ label, at }) => {
        const today = at.toDateString() === new Date().toDateString();
        const time = at.toLocaleString(
          i18n.language,
          compact
            ? {
                ...(today
                  ? {}
                  : { month: "short" as const, day: "numeric" as const }),
                hour: "2-digit",
                minute: "2-digit",
              }
            : undefined,
        );
        return `${label} · ${t("codexProvider.resets", { time })}`;
      })
      .join(compact ? " · " : "\n");
  return (
    <article
      className="tray-quota-card"
      data-many={items.length > 4 || undefined}
      aria-label={name}
      aria-busy={fetching}
      title={title ?? name}
    >
      <div className="tray-quota-who" title={title ?? name}>
        {name}
      </div>
      <div className="tray-quota-line">
        <span title={resetText(false)}>
          {status
            ? t(status)
            : resetText(true) ||
              t(
                items.length &&
                  items.every(({ tier }) => tier.utilization === 0)
                  ? "quotaPanel.unused"
                  : "trayPanel.quotaCurrent",
              )}
        </span>
        <button
          type="button"
          className="tray-quota-refresh"
          aria-label={`${t("codexProvider.refreshQuota")} ${name}`}
          disabled={disabled || fetching}
          aria-busy={fetching}
          onClick={refresh}
        >
          <RefreshCw
            size={12}
            aria-hidden="true"
            className={
              fetching ? "animate-spin motion-reduce:animate-none" : undefined
            }
          />
        </button>
      </div>
      {loading ? (
        <div className="tray-quota-rings" aria-hidden="true">
          <span className="tray-quota-skeleton" />
          <span className="tray-quota-skeleton" />
        </div>
      ) : (
        items.length > 0 && (
          <div className="tray-quota-rings">
            {items.map(({ tier, label }, index) => {
              const used = Math.max(0, Math.min(100, tier.utilization));
              const percent =
                Math.round((card.remaining ? 100 - used : used) * 10) / 10;
              const family = tier.name.includes(" · ")
                ? /^(.*) (\d+(?:mo|m|h|d|w))$/.exec(label)
                : null;
              return (
                <button
                  type="button"
                  className="tray-quota-ring"
                  data-low={used >= 90 || undefined}
                  key={`${tier.name}-${index}`}
                  onClick={card.toggle}
                  aria-label={`${label} · ${percent}% · ${t(card.remaining ? "trayPanel.quotaRemaining" : "trayPanel.quotaUsed")}`}
                  title={`${label}${
                    tier.resetsAt && Number.isFinite(Date.parse(tier.resetsAt))
                      ? `\n${t("codexProvider.resets", {
                          time: new Date(tier.resetsAt).toLocaleString(
                            i18n.language,
                          ),
                        })}`
                      : ""
                  }`}
                >
                  <span
                    className="tray-quota-dial"
                    role="meter"
                    aria-label={label}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={percent}
                  >
                    <svg viewBox="0 0 34 34" aria-hidden="true">
                      <circle
                        className="tray-quota-track"
                        cx="17"
                        cy="17"
                        r="15.5"
                      />
                      <circle
                        className="tray-quota-fill"
                        cx="17"
                        cy="17"
                        r="15.5"
                        pathLength="100"
                        strokeDasharray={`${percent} 100`}
                      />
                    </svg>
                    <b>{percent}%</b>
                  </span>
                  <span className="tray-quota-window" title={label}>
                    {family ? (
                      <>
                        <span>{poolLabel(family[1])}</span>
                        <span>{family[2]}</span>
                      </>
                    ) : (
                      ((
                        {
                          five_hour: "5h",
                          seven_day: "7d",
                          weekly: "7d",
                          daily: "1d",
                          thirty_day: "30d",
                        } as Record<string, string>
                      )[tier.name] ?? label)
                    )}
                  </span>
                </button>
              );
            })}
          </div>
        )
      )}
      {sample && items[0] && (
        <QuotaSparkline
          scope={sample.scope}
          sampledAt={sample.at}
          tier={items[0].tier}
          label={items[0].label}
        />
      )}
      {children && <div className="tray-quota-details">{children}</div>}
    </article>
  );
}
