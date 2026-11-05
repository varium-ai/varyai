// Adapted from Magpie's routing dur formatter (MIT); see third_party/routing-reference/LICENSE.
import type { TFunction } from "i18next";

// Match Magpie's routing countdown precision, including its rounded 48-hour cutoff.
export function formatRoutingDuration(ms: number, t: TFunction): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) return t("routingPage.duration.seconds", { n: seconds });
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("routingPage.duration.minutes", { n: minutes });
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  if (hours < 10 && remainder)
    return t("routingPage.duration.hoursMinutes", {
      hours,
      minutes: remainder,
    });
  if (hours < 48)
    return t("routingPage.duration.hours", { n: Math.round(minutes / 60) });
  return t("routingPage.duration.days", { n: Math.round(minutes / 1440) });
}
