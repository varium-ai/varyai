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

function CalendarCheckinProgress({
  totalDays,
  day,
  checkinDates,
  partialHistory = false,
  hint = "dailyCheckin.calendarHint",
  currentLabel = "dailyCheckin.today",
}: {
  totalDays?: number | null;
  day?: string;
  checkinDates?: string[] | null;
  partialHistory?: boolean;
  hint?: string;
  currentLabel?: string;
}) {
  const { t, i18n } = useTranslation();
  const today = day ? new Date(`${day}T00:00:00Z`) : null;
  const known = checkinDates != null;
  const dates = new Set(checkinDates);
  return (
    <div className="mp-checkin-progress">
      <div className="mp-checkin-streak">
        <h4>{t("dailyCheckin.campaignTotal")}</h4>
        <p className="mp-checkin-streak-value" data-known={totalDays != null}>
          {totalDays != null
            ? t("dailyCheckin.days", { count: totalDays })
            : t("dailyCheckin.noData")}
        </p>
        <p className="mp-checkin-muted">{day}</p>
      </div>
      <div className="mp-checkin-week">
        <h4>
          {t("dailyCheckin.calendar")}
          <span title={t(hint)}>
            <Info size={13} aria-label={t(hint)} />
          </span>
        </h4>
        <ol aria-label={t("dailyCheckin.calendar")}>
          {Array.from({ length: 7 }, (_, index) => {
            const date =
              today && new Date(today.getTime() - (6 - index) * 86400000);
            const iso = date?.toISOString().slice(0, 10);
            const state =
              !known || !iso
                ? "unknown"
                : dates.has(iso)
                  ? "complete"
                  : partialHistory
                    ? "unknown"
                    : "pending";
            return (
              <li
                key={index}
                data-date={iso}
                data-state={state}
                aria-current={index === 6 ? "date" : undefined}
              >
                <span className="mp-checkin-step" aria-hidden="true">
                  {state === "complete" && <Check size={12} strokeWidth={3} />}
                </span>
                <span className="sr-only">{t(`dailyCheckin.${state}`)} </span>
                <time dateTime={iso} title={iso}>
                  {date
                    ? date.toLocaleDateString(i18n?.language, {
                        month: "numeric",
                        day: "numeric",
                        timeZone: "UTC",
                      })
                    : "—"}
                </time>
                {index === 6 && <span>{t(currentLabel)}</span>}
              </li>
            );
          })}
        </ol>
      </div>
    </div>
  );
}

export function WorkBuddyCheckinProgress(props: {
  totalDays?: number | null;
  day?: string;
  checkinDates?: string[] | null;
}) {
  return <CalendarCheckinProgress {...props} />;
}

export function QoderCheckinProgress({
  day,
  confirmedDates,
}: {
  day?: string | null;
  confirmedDates?: string[];
}) {
  // Qoder's reward day changes at 10:00 UTC+8, not at local midnight.
  const current =
    day ?? new Date(Date.now() - 2 * 3600000).toISOString().slice(0, 10);
  return (
    <CalendarCheckinProgress
      day={current}
      checkinDates={confirmedDates}
      partialHistory
      hint="agentRewards.qoderCalendarHint"
      currentLabel="agentRewards.currentWindow"
    />
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
