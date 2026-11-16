export type DesktopAnalyticsEvent =
  | "desktop_install_observed"
  | "desktop_daily_active"
  | "desktop_version_changed";

export interface AnalyticsReport {
  token: string;
  measurementId: string;
  installationId: string;
  appVersion: string;
  os: string;
  arch: string;
  observedDate: string;
  events: DesktopAnalyticsEvent[];
}

declare global {
  interface Window {
    gtag?: (...args: unknown[]) => void;
    dataLayer?: unknown[];
    varyAnalytics?: ReturnType<typeof createAnalyticsReporter>;
  }
}

const allowedEvents = new Set<DesktopAnalyticsEvent>([
  "desktop_install_observed",
  "desktop_daily_active",
  "desktop_version_changed",
]);

function validReport(report: AnalyticsReport): boolean {
  return (
    typeof report.token === "string" &&
    report.token.length > 0 &&
    /^G-[A-Z0-9]+$/.test(report.measurementId) &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      report.installationId,
    ) &&
    typeof report.appVersion === "string" &&
    report.appVersion.length <= 64 &&
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(
      report.appVersion,
    ) &&
    ["macos", "windows", "linux"].includes(report.os) &&
    ["aarch64", "x86_64"].includes(report.arch) &&
    /^\d{4}-\d{2}-\d{2}$/.test(report.observedDate) &&
    Array.isArray(report.events) &&
    report.events.length > 0 &&
    report.events.length <= 2 &&
    report.events.every((event) => allowedEvents.has(event))
  );
}

// This module runs only in the isolated analytics WebView, never in the app UI.
export function createAnalyticsReporter(
  runtime: Window,
  document: Document,
  acknowledge: (token: string, dispatched: boolean) => Promise<unknown>,
) {
  let loading: Promise<void> | undefined;
  let configuredId: string | undefined;
  const tokens = new Set<string>();

  const load = (id: string) => {
    if (configuredId && configuredId !== id) return Promise.reject();
    configuredId = id;
    if (!loading) {
      runtime.dataLayer ??= [];
      runtime.gtag ??= function (..._args: unknown[]) {
        runtime.dataLayer!.push(arguments);
      };
      runtime.gtag("consent", "default", {
        analytics_storage: "granted",
        ad_storage: "denied",
        ad_user_data: "denied",
        ad_personalization: "denied",
      });
      loading = new Promise<void>((resolve, reject) => {
        const script = document.createElement("script");
        script.async = true;
        script.referrerPolicy = "no-referrer";
        script.src = `https://www.googletagmanager.com/gtag/js?id=${id}`;
        script.onload = () => resolve();
        script.onerror = () => reject();
        document.head.append(script);
      });
    }
    return loading;
  };

  let initialized = false;
  return {
    async report(report: AnalyticsReport) {
      if (!validReport(report) || tokens.has(report.token)) return;
      tokens.add(report.token);
      try {
        await load(report.measurementId);
        const tag = runtime.gtag!;
        if (!initialized) {
          tag("js", new Date());
          const id = report.installationId.replaceAll("-", "");
          const clientId = `${Number.parseInt(id.slice(0, 8), 16) || 1}.${Number.parseInt(id.slice(-8), 16) || 1}`;
          tag("config", report.measurementId, {
            client_id: clientId,
            send_page_view: false,
            allow_google_signals: false,
            allow_ad_personalization_signals: false,
            cookie_domain: "none",
            page_location: "https://www.varyai.net/desktop",
            page_title: "Vary AI desktop",
            page_referrer: "",
          });
          initialized = true;
        }
        await Promise.all(
          report.events.map(
            (event) =>
              new Promise<void>((resolve) => {
                tag("event", event, {
                  app_version: report.appVersion,
                  desktop_os: report.os,
                  desktop_arch: report.arch,
                  statistics_date: report.observedDate,
                  send_to: report.measurementId,
                  event_callback: resolve,
                });
              }),
          ),
        );
        // A gtag callback confirms dispatch, not ingestion or report availability.
        await acknowledge(report.token, true);
      } catch {
        await acknowledge(report.token, false).catch(() => {});
      }
    },
  };
}
