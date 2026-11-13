import { useTranslation } from "react-i18next";
import "./provider-loading.css";

export function ProviderQuotaSkeleton() {
  return (
    <span className="pv-quota-skeleton" aria-hidden="true">
      <span className="pv-skeleton pv-skeleton-window" />
      <span className="pv-skeleton pv-skeleton-window" />
    </span>
  );
}

export function ProviderListSkeleton() {
  const { t } = useTranslation();
  return (
    <div
      className="pv-list-skeleton"
      role="status"
      aria-label={t("common.loading")}
      aria-busy="true"
    >
      <span className="sr-only">{t("common.loading")}</span>
      <div className="pv-head" aria-hidden="true">
        <span className="pv-skeleton pv-skeleton-search" />
        <span className="pv-skeleton pv-skeleton-manage" />
        <span className="pv-search-scope">
          <span className="pv-skeleton pv-skeleton-hint" />
        </span>
      </div>
      <div className="mp-list pv-list" aria-hidden="true">
        {Array.from({ length: 6 }, (_, index) => (
          <div className="pv-row pv-skeleton-row" key={index}>
            <div className="pv-open">
              <span className="pv-skeleton pv-logo" />
              <span className="pv-who">
                <span className="pv-name">
                  <span className="pv-skeleton pv-skeleton-name" />
                </span>
                <span className="pv-sub">
                  <span className="pv-skeleton pv-skeleton-subtitle" />
                </span>
              </span>
            </div>
            <span className="pv-skeleton-key">
              <span className="pv-skeleton" />
            </span>
            <span className="pv-skeleton pv-skeleton-toggle" />
            <span className="pv-skeleton pv-skeleton-action" />
            <span className="pv-skeleton pv-skeleton-action" />
            <span className="pv-skeleton pv-skeleton-chevron" />
          </div>
        ))}
      </div>
    </div>
  );
}
