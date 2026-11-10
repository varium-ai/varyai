import { useSyncExternalStore } from "react";
import {
  useQueries,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { settingsApi, type AgentVersion } from "@/lib/api/settings";
import { compareVersions, isUpdateAvailable } from "@/lib/version";
import type { AppId } from "@/lib/api";
import { proxyApi } from "@/lib/api/proxy";

const cliTools: Partial<Record<AppId, string>> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
  grokbuild: "grok",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
  qwen: "qwen",
};
const desktopUpdateFailures = new Map([
  ["unavailable", "download"],
  ["download", "download"],
  ["checksum", "verification"],
  ["identity", "verification"],
  ["signature", "verification"],
  ["architecture", "verification"],
  ["archive", "archive"],
  ["running", "running"],
  ["replace", "replace"],
  ["restore", "restore"],
  ["changed", "changed"],
  ["unsupported", "unsupported"],
  ["notInstalled", "notInstalled"],
]);
export type AgentUpdateResult = {
  app: AppId;
  from?: string | null;
  to?: string | null;
  outcome: "changed" | "failed" | "unverified" | "preflightFailed";
  failureReason?: string;
};
const cliKey = (tool: string) => ["agent-cli-version", tool];
const desktopKey = ["agent-desktop-versions"];

type LifecycleState = {
  busy: boolean;
  reserved: AppId[];
  active: AppId[];
  progress: { index: number; total: number };
  pending: {
    apps: AppId[];
    ambiguous: AppId[];
  } | null;
  results: AgentUpdateResult[];
  barShown: boolean;
  installTarget: {
    app: AppId;
    website: string;
  } | null;
  installing: AppId | null;
  installResult: {
    app: AppId;
    outcome:
      | "installed"
      | "failed"
      | "unverified"
      | "checkFailed"
      | "alreadyInstalled"
      | "websiteOpened";
  } | null;
};
function createLifecycleStore() {
  let state: LifecycleState = {
    busy: false,
    reserved: [],
    active: [],
    progress: { index: 0, total: 0 },
    pending: null,
    results: [],
    barShown: false,
    installTarget: null,
    installing: null,
    installResult: null,
  };
  const listeners = new Set<() => void>();
  return {
    locked: { current: false },
    executing: { current: false },
    snapshot: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    set: <K extends keyof LifecycleState>(key: K, value: LifecycleState[K]) => {
      state = { ...state, [key]: value };
      listeners.forEach((listener) => listener());
    },
  };
}
// Keep tasks and their guards alive when the workspace page unmounts.
const lifecycleStores = new WeakMap<
  QueryClient,
  ReturnType<typeof createLifecycleStore>
>();

export function useAgentUpdates(installed: AppId[]) {
  const client = useQueryClient();
  let existingStore = lifecycleStores.get(client);
  if (!existingStore) {
    existingStore = createLifecycleStore();
    lifecycleStores.set(client, existingStore);
  }
  const store = existingStore;
  const state = useSyncExternalStore(store.subscribe, store.snapshot);
  const { locked, executing } = store;
  const {
    busy,
    active,
    progress,
    pending,
    results,
    barShown,
    installTarget,
    installing,
    installResult,
  } = state;
  const setBusy = (value: LifecycleState["busy"]) => store.set("busy", value);
  const setActive = (value: LifecycleState["active"]) =>
    store.set("active", value);
  const setProgress = (value: LifecycleState["progress"]) =>
    store.set("progress", value);
  const setPending = (value: LifecycleState["pending"]) =>
    store.set("pending", value);
  const setResults = (value: LifecycleState["results"]) =>
    store.set("results", value);
  const setBarShown = (value: LifecycleState["barShown"]) =>
    store.set("barShown", value);
  const setInstallTarget = (value: LifecycleState["installTarget"]) =>
    store.set("installTarget", value);
  const setInstalling = (value: LifecycleState["installing"]) =>
    store.set("installing", value);
  const setInstallResult = (value: LifecycleState["installResult"]) =>
    store.set("installResult", value);
  const apps = installed.filter((app) => !!cliTools[app]);
  const queries = useQueries({
    queries: apps.map((app) => ({
      queryKey: cliKey(cliTools[app]!),
      queryFn: async () =>
        (await settingsApi.getToolVersions([cliTools[app]!])).find(
          (v) => v.name === cliTools[app],
        ) ?? null,
      staleTime: 300_000,
      retry: false,
      refetchOnWindowFocus: false,
    })),
  });
  const cli = new Map(apps.map((app, i) => [app, queries[i].data]));
  const desktop = useQuery({
    queryKey: desktopKey,
    queryFn: settingsApi.getAgentVersions,
    staleTime: 300_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const updatable = installed.filter((app) => {
    const info = desktop.data?.[app];
    return cliTools[app]
      ? isUpdateAvailable(cli.get(app)?.version, cli.get(app)?.latest_version)
      : info?.supported &&
          info.installed &&
          info.source === "desktop" &&
          isUpdateAvailable(info.version, info.latest_version);
  });

  function unlock() {
    locked.current = false;
    setBusy(false);
    store.set("reserved", []);
    setActive([]);
  }
  async function execute(selected: AppId[]) {
    if (executing.current) return;
    executing.current = true;
    setPending(null);
    const completed: AgentUpdateResult[] = [];
    try {
      setActive(selected);
      await Promise.allSettled(
        selected.map(async (app) => {
          const tool = cliTools[app];
          const from = tool
            ? cli.get(app)?.version
            : desktop.data?.[app]?.version;
          let failed = false;
          let failureReason: string | undefined;
          try {
            if (tool) {
              await settingsApi.runToolLifecycleAction([tool], "update");
            } else {
              await settingsApi.updateDesktopAgent(app);
            }
          } catch (cause) {
            failed = true;
            // Only known desktop error codes are safe to show; CLI output stays private.
            if (!tool && typeof cause === "string") {
              failureReason = desktopUpdateFailures.get(cause);
            } else if (
              tool &&
              typeof cause === "string" &&
              cause.includes("from untrusted tap ")
            ) {
              failureReason = "homebrewTrust";
            }
          }
          let to: string | null | undefined;
          if (tool) {
            try {
              const fresh = (await settingsApi.getToolVersions([tool])).find(
                (v) => v.name === tool,
              );
              client.setQueryData(cliKey(tool), fresh ?? null);
              to = fresh?.version;
            } catch {
              // Never retain a stale version as evidence that the upgrade succeeded.
              client.setQueryData(cliKey(tool), null);
            }
          } else {
            let fresh: AgentVersion | undefined;
            try {
              fresh = (await settingsApi.getAgentVersions())[app];
              if (
                fresh?.supported &&
                fresh.installed &&
                fresh.source === "desktop"
              ) {
                to = fresh.version;
              }
            } catch {
              // A failed post-update probe must clear the selected app's stale snapshot.
            }
            client.setQueryData<Record<string, AgentVersion>>(
              desktopKey,
              (current) => {
                const next = { ...current };
                if (fresh) next[app] = fresh;
                else delete next[app];
                return next;
              },
            );
          }
          const verified = !!to && !!from && compareVersions(to, from) > 0;
          completed.push({
            app,
            from,
            to,
            outcome: failed ? "failed" : verified ? "changed" : "unverified",
            failureReason,
          });
          setResults(
            selected.flatMap((id) =>
              completed.filter((result) => result.app === id),
            ),
          );
          setActive(store.snapshot().active.filter((id) => id !== app));
          store.set(
            "reserved",
            store.snapshot().reserved.filter((id) => id !== app),
          );
          setProgress({ index: completed.length, total: selected.length });
        }),
      );
    } finally {
      executing.current = false;
      unlock();
      void client.invalidateQueries({ queryKey: ["agent-installations"] });
    }
  }
  async function start(requested: AppId[]) {
    if (locked.current) return;
    const selected = [...new Set(requested)].filter((app) =>
      updatable.includes(app),
    );
    if (!selected.length) return;
    // The ref also guards the preflight and confirmation windows before React commits.
    locked.current = true;
    // Reserve the selected tools before preflight so connection writes cannot race it.
    store.set("reserved", selected);
    setBusy(true);
    setBarShown(true);
    setResults([]);
    setProgress({ index: 0, total: selected.length });
    try {
      const selectedCli = selected.filter((app) => !!cliTools[app]);
      const plans = selectedCli.length
        ? await settingsApi.probeToolInstallations(
            selectedCli.map((app) => cliTools[app]!),
          )
        : [];
      const ambiguous = selectedCli.filter((app) => {
        const plan = plans.find((p) => p.tool === cliTools[app]);
        return !plan || plan.needs_confirmation || !plan.anchored;
      });
      if (ambiguous.length) {
        setPending({ apps: selected, ambiguous });
        return;
      }
    } catch {
      setResults(selected.map((app) => ({ app, outcome: "preflightFailed" })));
      unlock();
      return;
    }
    await execute(selected);
  }
  function requestInstall(app: AppId, website: string) {
    if (locked.current) return;
    locked.current = true;
    setBusy(true);
    setInstallResult(null);
    setInstallTarget({ app, website });
  }
  async function confirmInstall() {
    if (!installTarget || executing.current) return;
    executing.current = true;
    const { app, website } = installTarget;
    const tool = cliTools[app];
    setInstallTarget(null);
    setInstalling(app);
    try {
      if (!tool) {
        try {
          await settingsApi.openExternal(website);
          setInstallResult({ app, outcome: "websiteOpened" });
        } catch {
          setInstallResult({ app, outcome: "failed" });
        }
        return;
      }
      let present: Record<string, boolean>;
      try {
        present = await proxyApi.getAgentInstallations();
        client.setQueryData(["agent-installations"], present);
        if (typeof present[app] !== "boolean")
          throw new Error("Unknown installation state");
      } catch {
        setInstallResult({ app, outcome: "checkFailed" });
        return;
      }
      if (present[app]) {
        setInstallResult({ app, outcome: "alreadyInstalled" });
        return;
      }
      let failed = false;
      try {
        await settingsApi.runToolLifecycleAction([tool], "install");
      } catch {
        failed = true;
      }
      // An exit code alone is not evidence that the selected CLI is usable.
      const [presence, versions] = await Promise.allSettled([
        proxyApi.getAgentInstallations(),
        settingsApi.getToolVersions([tool]),
      ]);
      if (presence.status === "fulfilled")
        client.setQueryData(["agent-installations"], presence.value);
      const version =
        versions.status === "fulfilled"
          ? versions.value.find((v) => v.name === tool)
          : null;
      client.setQueryData(cliKey(tool), version ?? null);
      const verified =
        presence.status === "fulfilled" &&
        presence.value[app] &&
        !!version?.version &&
        !version.installed_but_broken;
      setInstallResult({
        app,
        outcome: failed ? "failed" : verified ? "installed" : "unverified",
      });
    } finally {
      executing.current = false;
      setInstalling(null);
      unlock();
    }
  }
  return {
    cli,
    desktop: desktop.data,
    updatable,
    busy,
    isBusy: (app: AppId | null) => {
      const state = store.snapshot();
      return (
        app !== null &&
        (state.reserved.includes(app) ||
          state.installTarget?.app === app ||
          state.installing === app)
      );
    },
    active,
    progress,
    pending,
    results,
    showBar: barShown || updatable.length >= 2,
    start,
    installTarget,
    installing,
    installResult,
    requestInstall,
    confirmInstall,
    canInstallCli: (app: AppId) => !!cliTools[app],
    cancelInstall: () => {
      if (executing.current) return;
      setInstallTarget(null);
      unlock();
    },
    confirm: () => {
      if (pending) void execute(pending.apps);
    },
    cancel: () => {
      if (executing.current) return;
      setPending(null);
      unlock();
    },
  };
}
