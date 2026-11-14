import type { ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { QuotaTier } from "@/types/subscription";
import { QuotaSparkline } from "./QuotaSparkline";

export interface QuotaCardOptions {
  remaining: boolean;
  toggle: () => void;
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
    tier.resetsAt && Number.isFinite(Date.parse(tier.resetsAt))
      ? [
          `${label} · ${t("codexProvider.resets", {
            time: new Date(tier.resetsAt).toLocaleString(i18n.language),
          })}`,
        ]
      : [],
  );
  return (
    <article
      className="tray-quota-card"
      aria-label={name}
      aria-busy={fetching}
      title={title ?? name}
    >
      <div className="tray-quota-info">
        <div className="tray-quota-who">{name}</div>
        <div className="tray-quota-line">
          <span title={resets.join("\n")}>
            {status ? t(status) : (resets[0] ?? t("trayPanel.quotaCurrent"))}
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
                  title={
                    tier.resetsAt && Number.isFinite(Date.parse(tier.resetsAt))
                      ? t("codexProvider.resets", {
                          time: new Date(tier.resetsAt).toLocaleString(
                            i18n.language,
                          ),
                        })
                      : undefined
                  }
                >
                  <span
                    className="tray-quota-dial"
                    role="meter"
                    aria-label={label}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={percent}
                  >
                    <svg viewBox="0 0 36 36" aria-hidden="true">
                      <circle
                        className="tray-quota-track"
                        cx="18"
                        cy="18"
                        r="15"
                      />
                      <circle
                        className="tray-quota-fill"
                        cx="18"
                        cy="18"
                        r="15"
                        pathLength="100"
                        strokeDasharray={`${percent} 100`}
                      />
                    </svg>
                    <b>{percent}%</b>
                  </span>
                  <span className="tray-quota-window" title={label}>
                    {family ? (
                      <>
                        <span>{family[1]}</span>
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
