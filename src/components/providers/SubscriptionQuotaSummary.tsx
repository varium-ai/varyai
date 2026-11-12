import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { QuotaTier } from "@/types/subscription";

export function SubscriptionQuotaSummary({
  label,
  items,
  status,
  fetching,
  children,
}: {
  label: string;
  items: Array<{ tier: QuotaTier; label: string }>;
  status: string | null;
  fetching: boolean;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <span
      className="pv-quota-summary"
      role="status"
      aria-label={label}
      aria-busy={fetching}
    >
      {items.map(({ tier, label }, index) => (
        <span className="pv-quota-window" key={`${tier.name}-${index}`}>
          {label}{" "}
          <b>
            {tier.display ??
              t("codexProvider.remaining", {
                n: Math.max(
                  0,
                  Math.min(100, Math.round((100 - tier.utilization) * 10) / 10),
                ),
              })}
          </b>
        </span>
      ))}
      {children}
      {status && <span>{t(status)}</span>}
    </span>
  );
}
