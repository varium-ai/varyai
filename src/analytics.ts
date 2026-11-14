import { invoke } from "@tauri-apps/api/core";
import { createAnalyticsReporter } from "./lib/analytics";

window.varyAnalytics = createAnalyticsReporter(
  window,
  document,
  (token, dispatched) =>
    invoke("acknowledge_analytics_report", { token, dispatched }),
);
