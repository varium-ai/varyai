import type { ReactNode } from "react";
import { Check, Info, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export function CheckinProgress({
  streak,
  current,
}: {
  streak?: number | null;
  current?: { complete: boolean; label: string };
}) {
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
            const isCurrent = current !== undefined && index === 6;
            const complete = isCurrent
              ? current.complete
              : known &&
                (current?.complete ? index >= 7 - streak : index < streak);
            const state = complete ? "complete" : known ? "pending" : "unknown";
            return (
              <li key={index} data-state={state}>
                <span className="mp-checkin-step" aria-hidden="true">
                  {state === "complete" && <Check size={12} strokeWidth={3} />}
                </span>
                <span className="sr-only">{t(`dailyCheckin.${state}`)} </span>
                <span>
                  {isCurrent
                    ? t(current.label)
                    : t("dailyCheckin.day", { count: index + 1 })}
                </span>
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
  todayConfirmed = false,
  localHistory = false,
  hint = "dailyCheckin.calendarHint",
  currentLabel = "dailyCheckin.today",
}: {
  totalDays?: number | null;
  day?: string;
  checkinDates?: string[] | null;
  todayConfirmed?: boolean;
  localHistory?: boolean;
  hint?: string;
  currentLabel?: string;
}) {
  const { t, i18n } = useTranslation();
  const today = day ? new Date(`${day}T00:00:00Z`) : null;
  const known = checkinDates != null;
  const dates = new Set(checkinDates);
  const displayedDates = Array.from(
    { length: 7 },
    (_, index) => today && new Date(today.getTime() - (6 - index) * 86400000),
  );
  const total = localHistory
    ? displayedDates.filter(
        (date) => date && dates.has(date.toISOString().slice(0, 10)),
      ).length || undefined
    : totalDays;
  return (
    <div className="mp-checkin-progress">
      <div className="mp-checkin-streak">
        <h4>
          {t(
            localHistory
              ? "agentRewards.confirmedLastSevenDays"
              : "dailyCheckin.campaignTotal",
          )}
        </h4>
        <p className="mp-checkin-streak-value" data-known={total != null}>
          {total != null
            ? t("dailyCheckin.days", { count: total })
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
          {displayedDates.map((date, index) => {
            const iso = date?.toISOString().slice(0, 10);
            const complete =
              iso && (dates.has(iso) || (index === 6 && todayConfirmed));
            const state = complete
              ? "complete"
              : known && iso && !localHistory
                ? "pending"
                : "unknown";
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
  todayConfirmed?: boolean;
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
      localHistory
      hint="agentRewards.qoderCalendarHint"
      currentLabel="agentRewards.currentWindow"
    />
  );
}

export function TraeCheckinProgress({
  confirmedUntil,
}: {
  confirmedUntil?: number | null;
}) {
  const utc8Offset = 8 * 3_600_000;
  const dayLength = 86_400_000;
  const todayStart =
    Math.floor((Date.now() + utc8Offset) / dayLength) * dayLength;
  const day = new Date(todayStart).toISOString().slice(0, 10);
  const nextReset = todayStart + dayLength - utc8Offset;
  return (
    <CalendarCheckinProgress
      day={day}
      checkinDates={confirmedUntil === nextReset ? [day] : []}
      localHistory
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
