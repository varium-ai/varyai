import { useRef, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api/settings";
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
export type AgentUpdateResult = {
  app: AppId;
  from?: string | null;
  to?: string | null;
  outcome: "changed" | "failed" | "unverified" | "preflightFailed";
};
const cliKey = (tool: string) => ["agent-cli-version", tool];

export function useAgentUpdates(installed: AppId[]) {
  const client = useQueryClient();
  const locked = useRef(false);
  const executing = useRef(false);
  const [busy, setBusy] = useState(false);
  const [current, setCurrent] = useState<AppId | null>(null);
  const [progress, setProgress] = useState({ index: 0, total: 0 });
  const [pending, setPending] = useState<{
    apps: AppId[];
    ambiguous: AppId[];
  } | null>(null);
  const [results, setResults] = useState<AgentUpdateResult[]>([]);
  const [barShown, setBarShown] = useState(false);
  const [installTarget, setInstallTarget] = useState<{
    app: AppId;
    website: string;
  } | null>(null);
  const [installing, setInstalling] = useState<AppId | null>(null);
  const [installResult, setInstallResult] = useState<{
    app: AppId;
    outcome:
      | "installed"
      | "failed"
      | "unverified"
      | "checkFailed"
      | "alreadyInstalled"
      | "websiteOpened";
  } | null>(null);
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
    queryKey: ["agent-desktop-versions"],
    queryFn: settingsApi.getAgentVersions,
    staleTime: 300_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const updatable = apps.filter((app) =>
    isUpdateAvailable(cli.get(app)?.version, cli.get(app)?.latest_version),
  );

  function unlock() {
    locked.current = false;
    setBusy(false);
    setCurrent(null);
  }
  async function execute(selected: AppId[]) {
    if (executing.current) return;
    executing.current = true;
    setPending(null);
    const completed: AgentUpdateResult[] = [];
    try {
      // Global package managers must never write concurrently, even for different tools.
      for (const [index, app] of selected.entries()) {
        setCurrent(app);
        setProgress({ index: index + 1, total: selected.length });
        const tool = cliTools[app]!;
        const from = cli.get(app)?.version;
        let failed = false;
        try {
          await settingsApi.runToolLifecycleAction([tool], "update");
        } catch {
          failed = true;
        }
        let fresh:
          | Awaited<ReturnType<typeof settingsApi.getToolVersions>>[number]
          | undefined;
        try {
          fresh = (await settingsApi.getToolVersions([tool])).find(
            (v) => v.name === tool,
          );
          client.setQueryData(cliKey(tool), fresh ?? null);
        } catch {
          // Never retain a stale version as evidence that the upgrade succeeded.
          client.setQueryData(cliKey(tool), null);
        }
        const verified =
          !!fresh?.version &&
          !!from &&
          compareVersions(fresh.version, from) > 0;
        completed.push({
          app,
          from,
          to: fresh?.version,
          outcome: failed ? "failed" : verified ? "changed" : "unverified",
        });
        setResults([...completed]);
      }
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
    setBusy(true);
    setBarShown(true);
    setResults([]);
    setProgress({ index: 0, total: selected.length });
    try {
      const plans = await settingsApi.probeToolInstallations(
        selected.map((app) => cliTools[app]!),
      );
      const ambiguous = selected.filter((app) => {
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
    current,
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
