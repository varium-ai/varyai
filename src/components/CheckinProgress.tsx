import type { ReactNode } from "react";
import { Check, Info, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export function CheckinProgress({ streak }: { streak?: number | null }) {
  const { t } = useTranslation();
  const known = streak != null;
  return (
    <div className="mp-checkin-progress">
      <div className="mp-checkin-streak">
        <h4>{t("dailyCheckin.streak")}</h4>
        <p className="mp-checkin-streak-value" data-known={known}>
          {known
            ? t("dailyCheckin.days", { count: streak })
            : t("dailyCheckin.noData")}
        </p>
        <p className="mp-checkin-muted">{new Date().toLocaleDateString()}</p>
      </div>
      <div className="mp-checkin-week">
        <h4>
          {t("dailyCheckin.progress")}
          <span title={t("dailyCheckin.progressHint")}>
            <Info size={13} aria-label={t("dailyCheckin.progressHint")} />
          </span>
        </h4>
        <ol aria-label={t("dailyCheckin.progress")}>
          {Array.from({ length: 7 }, (_, index) => {
            const state = !known
              ? "unknown"
              : index < streak
                ? "complete"
                : "pending";
            return (
              <li key={index} data-state={state}>
                <span className="mp-checkin-step" aria-hidden="true">
                  {state === "complete" && <Check size={12} strokeWidth={3} />}
                </span>
                <span className="sr-only">{t(`dailyCheckin.${state}`)} </span>
                <span>{t("dailyCheckin.day", { count: index + 1 })}</span>
              </li>
            );
          })}
        </ol>
      </div>
    </div>
  );
}

export function CheckinNotice() {
  const { t } = useTranslation();
  return (
    <p className="mp-checkin-notice">
      <Info size={13} aria-hidden="true" />
      {t("dailyCheckin.notice")}
    </p>
  );
}

export function CheckinAccountTools({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <details className="mp-checkin-manage">
      <summary role="button">
        {t("dailyCheckin.manage")}
        <Settings2 size={13} aria-hidden="true" />
      </summary>
      <div className="mp-checkin-manage-body">{children}</div>
    </details>
  );
}
