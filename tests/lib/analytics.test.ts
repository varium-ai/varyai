import { describe, expect, it, vi, beforeEach } from "vitest";
import { createAnalyticsReporter, type AnalyticsReport } from "@/lib/analytics";

const report: AnalyticsReport = {
  token: "test-dispatch-token",
  measurementId: "G-2R2MZ76G1E",
  installationId: "12f57d58-7019-4e84-8ed7-f9ce4731e3cb",
  appVersion: "0.3.0",
  os: "windows",
  arch: "x86_64",
  observedDate: "2026-11-13",
  events: ["desktop_install_observed", "desktop_daily_active"],
};

function setup() {
  const acknowledge = vi.fn().mockResolvedValue(true);
  const reporter = createAnalyticsReporter(window, document, acknowledge);
  return { acknowledge, reporter };
}

beforeEach(() => {
  document.head.innerHTML = "";
  delete window.gtag;
  delete window.dataLayer;
});

describe("isolated gtag reporter", () => {
  it("loads the public Google tag directly and sends only the allowed desktop events", async () => {
    const { reporter, acknowledge } = setup();
    const done = reporter.report(report);
    const script = document.querySelector("script")!;
    expect(script.src).toBe(
      "https://www.googletagmanager.com/gtag/js?id=G-2R2MZ76G1E",
    );
    const calls: unknown[][] = [];
    window.gtag = (...args: unknown[]) => {
      calls.push(args);
      if (args[0] === "event")
        (args[2] as { event_callback: () => void }).event_callback();
    };
    script.dispatchEvent(new Event("load"));
    await done;
    const config = calls.find(([name]) => name === "config")![2];
    expect(config).toMatchObject({
      client_id: "318078296.1194451915",
      send_page_view: false,
      page_location: "https://www.varyai.net/desktop",
      page_referrer: "",
      allow_google_signals: false,
    });
    expect(
      calls.filter(([name]) => name === "event").map(([, name]) => name),
    ).toEqual(report.events);
    const params = calls.find(([name]) => name === "event")![2] as Record<
      string,
      unknown
    >;
    expect(Object.keys(params).sort()).toEqual(
      [
        "app_version",
        "desktop_arch",
        "desktop_os",
        "event_callback",
        "send_to",
        "statistics_date",
      ].sort(),
    );
    expect(acknowledge).toHaveBeenCalledWith(report.token, true);
  });

  it("does not acknowledge dispatch when the script fails to load", async () => {
    const { reporter, acknowledge } = setup();
    const done = reporter.report(report);
    document.querySelector("script")!.dispatchEvent(new Event("error"));
    await done;
    expect(acknowledge).toHaveBeenCalledWith(report.token, false);
    expect(acknowledge).not.toHaveBeenCalledWith(report.token, true);
  });

  it("rejects arbitrary event names and incomplete reports before loading any remote script", async () => {
    const { reporter, acknowledge } = setup();
    await reporter.report({
      ...report,
      events: ["private_tool_input"],
    } as unknown as AnalyticsReport);
    await reporter.report({
      ...report,
      measurementId: "https://example.invalid/",
    });
    await reporter.report({ ...report, os: "private-value" });
    expect(document.querySelector("script")).toBeNull();
    expect(acknowledge).not.toHaveBeenCalledWith(report.token, true);
  });

  it("initializes one tag for repeated daily reports and acknowledges each token once", async () => {
    const { reporter, acknowledge } = setup();
    const first = reporter.report(report);
    window.gtag = (...args: unknown[]) => {
      if (args[0] === "event") {
        const cb = (args[2] as { event_callback: () => void }).event_callback;
        cb();
        cb();
      }
    };
    document.querySelector("script")!.dispatchEvent(new Event("load"));
    await first;
    await reporter.report({
      ...report,
      token: "next-day",
      observedDate: "2026-11-14",
      events: ["desktop_daily_active"],
    });
    expect(document.querySelectorAll("script")).toHaveLength(1);
    expect(acknowledge).toHaveBeenCalledTimes(2);
  });
});
