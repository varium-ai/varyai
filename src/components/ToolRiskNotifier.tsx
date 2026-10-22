import { useCallback, useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@/lib/api/notifications";
import { toolRiskKeys, useToolRiskEventBridge } from "@/hooks/useToolRisk";
import { toolRiskApi } from "@/lib/api";
import { reportFrontendError } from "@/lib/frontendLogger";
import {
  RISK_CATEGORIES,
  type ToolRiskEventRecord,
  type ToolRiskSettings,
} from "@/types/toolRisk";

const PAGE_SIZE = 50;
const PAGE_BUDGET = 4;
const CONTINUATION_DELAY_MS = 250;
const WINDOW_MS = 60_000;
const OS_GRACE_MS = 10_000;
const OS_QUEUE_SIZE = 2;
const SEVERITIES = ["none", "medium", "high"] as const;
type Category = (typeof RISK_CATEGORIES)[number] | "unknown";
type Severity = (typeof SEVERITIES)[number] | "unknown";
type Translate = ReturnType<typeof useTranslation>["t"];

// Only validated enums and bounded counters survive beyond the page read.
interface RiskWindow {
  id: number;
  generation: number;
  policyVersion: number;
  endsAt: number;
  count: number;
  category: Category;
  severity: Severity;
}
interface Delivery extends RiskWindow {
  summary: boolean;
}

function createCancellation() {
  return new AbortController();
}

// Remove the listener on both completion and cancellation. Unlike repeatedly
// racing one unresolved Promise, this does not retain handlers across windows.
function waitForTail(pending: Promise<void> | undefined, signal: AbortSignal) {
  if (!pending || signal.aborted) return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    const cleanup = () => signal.removeEventListener("abort", cancelled);
    const cancelled = () => {
      cleanup();
      resolve();
    };
    signal.addEventListener("abort", cancelled, { once: true });
    void pending.then(
      () => {
        cleanup();
        resolve();
      },
      (error: unknown) => {
        cleanup();
        reject(error);
      },
    );
  });
}

function createSession() {
  return {
    mountedAt: Date.now(),
    lastSeenId: 0,
    running: false,
    dirty: false,
    disposed: false,
    generation: 0,
    cancellation: createCancellation(),
    policy: null as ToolRiskSettings | null,
    window: undefined as RiskWindow | undefined,
    tailTimer: undefined as ReturnType<typeof setTimeout> | undefined,
    tailDelivery: undefined as Promise<void> | undefined,
    continuation: undefined as ReturnType<typeof setTimeout> | undefined,
    permission: undefined as Promise<boolean> | undefined,
    osRunning: false,
    osQueue: [] as Delivery[],
  };
}
type NotificationSession = ReturnType<typeof createSession>;

/** Only in-scope risky proposed calls notify; all other verdicts stay silent. */
export function shouldNotify(record: ToolRiskEventRecord): boolean {
  return record.detectionScope === "in_scope" && record.verdict === "risky";
}

function cancelPolicy(state: NotificationSession) {
  state.generation += 1;
  state.cancellation.abort();
  state.cancellation = createCancellation();
  clearTimeout(state.tailTimer);
  state.tailTimer = undefined;
  state.window = undefined;
  // Cancelled expiry IPC may finish later, but must not gate the new generation.
  state.tailDelivery = undefined;
  state.osQueue = [];
  // Never mark an uncancellable permission/send Promise as finished.
}

async function readSettings(state: NotificationSession) {
  if (state.disposed) return null;
  const generation = state.generation;
  try {
    const settings = await toolRiskApi.getSettings();
    if (state.disposed || state.generation !== generation) return null;
    if (
      state.policy &&
      (state.policy.enabled !== settings.enabled ||
        state.policy.policyVersion !== settings.policyVersion)
    )
      cancelPolicy(state);
    state.policy = settings;
    return settings.enabled ? settings : null;
  } catch {
    if (!state.disposed && state.generation === generation) {
      cancelPolicy(state);
      state.policy = null;
      reportFrontendError("tool_risk_notification_settings", "failed");
    }
    return null;
  }
}

function isLive(state: NotificationSession, item: Delivery) {
  return !state.disposed && state.generation === item.generation;
}
function isFresh(state: NotificationSession, item: Delivery) {
  return isLive(state, item) && performance.now() <= item.endsAt + OS_GRACE_MS;
}
async function isCurrentPolicy(state: NotificationSession, item: Delivery) {
  if (!isLive(state, item)) return false;
  const settings = await readSettings(state);
  return isLive(state, item) && settings?.policyVersion === item.policyVersion;
}

function formatDelivery(item: Delivery, t: Translate) {
  const category = t(
    item.category === "unknown"
      ? "toolRisk.notifyUnknownCategory"
      : `toolRisk.category.${item.category}`,
  );
  const severity = t(
    item.severity === "unknown"
      ? "toolRisk.notifyUnknownSeverity"
      : `toolRisk.severity.${item.severity}`,
  );
  return {
    title: t("toolRisk.notifyTitle"),
    body: t(
      item.summary ? "toolRisk.notifySummaryBody" : "toolRisk.notifyBody",
      {
        category,
        severity,
        count: item.count,
      },
    ),
  };
}

function getPermission(state: NotificationSession, item: Delivery) {
  // Exactly one permission task per mount, independent of ingestion and windows.
  state.permission ??= (async () => {
    try {
      const granted = await isPermissionGranted();
      if (state.disposed) return false;
      // Permission is mount-wide OS state, not a delivery for the old policy.
      // Every consumer still validates its own generation before sending.
      if (granted) return true;
      // The lookup may outlive its original window. Use only a currently queued
      // fresh item for the one permission request, never revive the old item.
      const current = state.osQueue.find((entry) => isFresh(state, entry));
      if (
        !current ||
        !(await isCurrentPolicy(state, current)) ||
        !state.osQueue.includes(current) ||
        !isFresh(state, current)
      )
        return false;
      const result = await requestPermission();
      if (state.disposed) return false;
      if (isLive(state, current) && result !== "granted")
        reportFrontendError("tool_risk_permission_denied", "denied");
      return result === "granted";
    } catch {
      if (isLive(state, item))
        reportFrontendError("tool_risk_permission", "failed");
      return false;
    }
  })();
  return state.permission;
}

function pumpOS(state: NotificationSession, translate: { current: Translate }) {
  if (state.disposed || state.osRunning || state.osQueue.length === 0) return;
  state.osRunning = true;
  void (async () => {
    try {
      while (!state.disposed && state.osQueue.length > 0) {
        const item = state.osQueue[0];
        if (!isFresh(state, item)) {
          state.osQueue.shift();
          continue;
        }
        const granted = await getPermission(state, item);
        // The queued item may have been cancelled or evicted during permission.
        if (state.osQueue[0] !== item) continue;
        state.osQueue.shift();
        if (!granted || !isFresh(state, item)) continue;
        if (!(await isCurrentPolicy(state, item)) || !isFresh(state, item))
          continue;
        try {
          await sendNotification(formatDelivery(item, translate.current));
        } catch {
          if (isLive(state, item))
            reportFrontendError("tool_risk_notification_send", "failed");
        }
      }
    } finally {
      state.osRunning = false;
    }
  })();
}

async function deliverApp(
  state: NotificationSession,
  item: Delivery,
  translate: { current: Translate },
) {
  // Cache signals only cancel; IPC is the authority before every delivery.
  if (!(await isCurrentPolicy(state, item)) || !isLive(state, item)) return;
  const { title, body } = formatDelivery(item, translate.current);
  try {
    toast.error(title, {
      description: body,
      closeButton: true,
      id: item.summary
        ? `tool-risk-window-${item.generation}-${item.id}`
        : `tool-risk-${item.id}`,
    });
  } catch {
    reportFrontendError("tool_risk_toast", "failed");
  }
  if (!isLive(state, item)) return;
  if (state.osQueue.length === OS_QUEUE_SIZE) {
    state.osQueue.shift();
    reportFrontendError("tool_risk_notification_queue", "dropped_oldest");
  }
  state.osQueue.push(item);
  pumpOS(state, translate);
}

function closeWindow(
  state: NotificationSession,
  translate: { current: Translate },
) {
  const window = state.window;
  clearTimeout(state.tailTimer);
  state.tailTimer = undefined;
  state.window = undefined;
  if (!window || window.count <= 1) return state.tailDelivery;
  // Snapshot the old window synchronously, before a new one can accept records.
  const pending = deliverApp(state, { ...window, summary: true }, translate);
  state.tailDelivery = pending;
  void pending.finally(() => {
    if (state.tailDelivery === pending) state.tailDelivery = undefined;
  });
  return pending;
}

async function acceptRecord(
  state: NotificationSession,
  record: ToolRiskEventRecord,
  translate: { current: Translate },
) {
  const generation = state.generation;
  const cancelled = state.cancellation.signal;
  if (state.window && performance.now() >= state.window.endsAt) {
    await waitForTail(closeWindow(state, translate), cancelled);
  } else if (state.tailDelivery) {
    await waitForTail(state.tailDelivery, cancelled);
  }
  if (state.disposed || state.generation !== generation) return;
  const category =
    RISK_CATEGORIES.find((value) => value === record.riskCategory) ?? "unknown";
  const severity =
    SEVERITIES.find((value) => value === record.severity) ?? "unknown";
  if (state.window) {
    state.window.count = Math.min(
      Number.MAX_SAFE_INTEGER,
      state.window.count + 1,
    );
    state.window.category = category;
    state.window.severity = severity;
    return;
  }
  const window: RiskWindow = {
    id: record.id,
    generation,
    policyVersion: record.policyVersion,
    endsAt: performance.now() + WINDOW_MS,
    count: 1,
    category,
    severity,
  };
  state.window = window;
  state.tailTimer = setTimeout(() => {
    if (
      !state.disposed &&
      state.generation === generation &&
      state.window === window
    ) {
      void closeWindow(state, translate);
    }
  }, WINDOW_MS);
  await deliverApp(state, { ...window, summary: false }, translate);
}

/** Payload-free signals trigger bounded, ascending-ID catch-up, never OS awaits. */
export function ToolRiskNotifier() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const translate = useRef(t);
  translate.current = t;
  const session = useRef(createSession());

  useEffect(() => {
    if (session.current.disposed) session.current = createSession();
    const state = session.current;
    const unsubscribe = queryClient.getQueryCache().subscribe((event) => {
      if (event.type !== "updated" || event.action.type !== "success") return;
      if (
        JSON.stringify(event.query.queryKey) !==
        JSON.stringify(toolRiskKeys.settings())
      )
        return;
      const settings = event.query.state.data as ToolRiskSettings | undefined;
      if (
        state.policy &&
        settings &&
        (!settings.enabled ||
          settings.policyVersion !== state.policy.policyVersion)
      ) {
        cancelPolicy(state);
        state.policy = null;
      }
    });
    return () => {
      state.disposed = true;
      cancelPolicy(state);
      clearTimeout(state.continuation);
      unsubscribe();
    };
  }, [queryClient]);

  const onRecorded = useCallback(function drain() {
    const state = session.current;
    if (state.disposed) return;
    state.dirty = true;
    if (state.running || state.continuation !== undefined) return;
    state.running = true;
    void (async () => {
      let more = false;
      try {
        for (let page = 0; page < PAGE_BUDGET && !state.disposed; page += 1) {
          state.dirty = false;
          more = false;
          const settings = await readSettings(state);
          if (!settings) {
            // dirty was cleared before this await: any true value now is a
            // genuinely newer signal. Preserve it, even for a disabled snapshot.
            return;
          }
          const generation = state.generation;
          const records = await toolRiskApi.getEventsAfter(
            state.lastSeenId,
            PAGE_SIZE,
            settings.policyVersion,
          );
          if (state.disposed || state.generation !== generation) return;
          const previousId = state.lastSeenId;
          for (const record of records) {
            if (state.disposed || state.generation !== generation) return;
            if (record.id <= state.lastSeenId) continue;
            state.lastSeenId = record.id;
            if (
              record.timestamp < state.mountedAt ||
              record.policyVersion !== settings.policyVersion ||
              !shouldNotify(record)
            )
              continue;
            const current = await readSettings(state);
            if (state.disposed || state.generation !== generation) return;
            if (current?.policyVersion !== record.policyVersion) continue;
            await acceptRecord(state, record, translate);
          }
          if (records.length > 0 && state.lastSeenId === previousId) {
            state.dirty = false;
            return;
          }
          more = records.length === PAGE_SIZE;
          if (!more && !state.dirty) return;
        }
      } catch {
        if (!state.disposed)
          reportFrontendError("tool_risk_notification_fetch", "failed");
        state.dirty = false;
        more = false;
      } finally {
        state.running = false;
        if (!state.disposed && (more || state.dirty)) {
          state.continuation = setTimeout(() => {
            state.continuation = undefined;
            if (!state.disposed) drain();
          }, CONTINUATION_DELAY_MS);
        }
      }
    })();
  }, []);

  useToolRiskEventBridge(onRecorded);
  return null;
}
