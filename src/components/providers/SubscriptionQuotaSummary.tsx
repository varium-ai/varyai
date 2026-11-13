import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { QuotaTier } from "@/types/subscription";
import { ProviderQuotaSkeleton } from "./ProviderLoading";

export function SubscriptionQuotaSummary({
  label,
  items,
  status,
  fetching,
  loading = false,
  children,
}: {
  label: string;
  items: Array<{ tier: QuotaTier; label: string }>;
  status: string | null;
  fetching: boolean;
  loading?: boolean;
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
      {loading && <ProviderQuotaSkeleton />}
      {items.map(({ tier, label }, index) => (
        <span className="pv-quota-window" key={`${tier.name}-${index}`}>
          {label}{" "}
          <b>
            {t("codexProvider.remaining", {
              n: Math.max(
                0,
                Math.min(100, Math.round((100 - tier.utilization) * 10) / 10),
              ),
            })}
          </b>
        </span>
      ))}
      {children}
      {status && (
        <span className={loading ? "sr-only" : undefined}>{t(status)}</span>
      )}
    </span>
  );
}
