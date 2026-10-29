import { useEffect, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { toolRiskApi } from "@/lib/api";
import {
  invalidateSecurityEventDomains,
  invalidateSecurityEventsQueries,
} from "@/hooks/useSecurityEvents";
import { reportFrontendError } from "@/lib/frontendLogger";
import type {
  SaveCommandListInput,
  SaveCustomRuleInput,
  SandboxTestInput,
  ToolRiskSettings,
  ToolRiskVerdict,
  ToolRiskWriteReceipt,
} from "@/types/toolRisk";

export const toolRiskKeys = {
  all: ["toolRisk"] as const,
  settings: () => [...toolRiskKeys.all, "settings"] as const,
  rules: () => [...toolRiskKeys.all, "rules"] as const,
  commandLists: () => [...toolRiskKeys.all, "commandLists"] as const,
  policyChanges: (limit: number, offset: number) =>
    [...toolRiskKeys.all, "policyChanges", limit, offset] as const,
  rule: (id: string) => [...toolRiskKeys.all, "rule", id] as const,
  events: (limit: number, offset: number, verdict?: ToolRiskVerdict) =>
    [...toolRiskKeys.all, "events", limit, offset, verdict ?? "all"] as const,
};

function useWriteNotice() {
  const { t } = useTranslation();
  return (receipt: ToolRiskWriteReceipt<unknown>, successKey?: string) => {
    const runtimeFailed = receipt.runtimeStatus === "apply_failed";
    const auditUnavailable = receipt.auditStatus === "audit_unavailable";
    if (runtimeFailed) toast.warning(t("toolRisk.savedNotApplied"));
    if (auditUnavailable) toast.warning(t("toolRisk.auditUnavailable"));
    if (!runtimeFailed && !auditUnavailable && successKey)
      toast.success(t(successKey));
  };
}

export function useToolRiskSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const writeNotice = useWriteNotice();

  const query = useQuery({
    queryKey: toolRiskKeys.settings(),
    queryFn: () => toolRiskApi.getSettings(),
  });

  const mutation = useMutation({
    mutationFn: (settings: ToolRiskSettings) =>
      toolRiskApi.updateSettings(settings),
    onSuccess: (receipt) => {
      void queryClient.invalidateQueries({ queryKey: toolRiskKeys.all });
      writeNotice(receipt, "toolRisk.settingsUpdated");
    },
    onError: () => toast.error(t("toolRisk.settingsUpdateFailed")),
  });

  return {
    settings: query.data,
    isLoading: query.isLoading,
    updateSettings: async (settings: ToolRiskSettings): Promise<void> => {
      await mutation.mutateAsync(settings);
    },
    isUpdating: mutation.isPending,
  };
}

export function useToolRiskRules() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const writeNotice = useWriteNotice();

  const query = useQuery({
    queryKey: toolRiskKeys.rules(),
    queryFn: () => toolRiskApi.getRules(),
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: toolRiskKeys.all });
  };

  const toggleMutation = useMutation({
    mutationFn: ({
      id,
      enabled,
      expectedRevision,
    }: {
      id: string;
      enabled: boolean;
      expectedRevision: number;
    }) => toolRiskApi.toggleRule(id, enabled, expectedRevision),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt);
    },
    onError: () => toast.error(t("toolRisk.policyWriteFailed")),
  });

  const resetMutation = useMutation({
    mutationFn: ({
      id,
      expectedRevision,
    }: {
      id: string;
      expectedRevision: number;
    }) => toolRiskApi.resetBuiltinRule(id, expectedRevision),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt);
    },
    onError: () => toast.error(t("toolRisk.policyWriteFailed")),
  });

  const saveMutation = useMutation({
    mutationFn: (input: SaveCustomRuleInput) =>
      toolRiskApi.saveCustomRule(input),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt, "toolRisk.ruleSaved");
    },
    onError: (err: unknown) => {
      toast.error(
        t("toolRisk.ruleSaveFailed") +
          (String(err).includes("tool_risk_builtin_duplicate")
            ? t("toolRisk.builtinDuplicate")
            : String(err).includes("tool_risk_risk_severity_required")
              ? t("toolRisk.riskSeverityRequired")
              : ""),
      );
    },
  });

  const deleteMutation = useMutation({
    mutationFn: ({
      id,
      expectedRevision,
    }: {
      id: string;
      expectedRevision: number;
    }) => toolRiskApi.deleteCustomRule(id, expectedRevision),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt, "toolRisk.ruleDeleted");
    },
    onError: () => toast.error(t("toolRisk.policyWriteFailed")),
  });

  return {
    rules: query.data ?? [],
    isLoading: query.isLoading,
    refetch: query.refetch,
    toggleRule: async (
      id: string,
      enabled: boolean,
      expectedRevision: number,
    ): Promise<void> => {
      await toggleMutation.mutateAsync({ id, enabled, expectedRevision });
    },
    resetBuiltinRule: async (
      id: string,
      expectedRevision: number,
    ): Promise<void> => {
      await resetMutation.mutateAsync({ id, expectedRevision });
    },
    saveRule: async (input: SaveCustomRuleInput) =>
      (await saveMutation.mutateAsync(input)).value,
    deleteRule: async (id: string, expectedRevision: number) =>
      (await deleteMutation.mutateAsync({ id, expectedRevision })).value,
    isSaving: saveMutation.isPending,
  };
}

export function useToolRiskEvents(
  limit: number,
  offset: number,
  verdict?: ToolRiskVerdict,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: toolRiskKeys.events(limit, offset, verdict),
    queryFn: () =>
      verdict === undefined
        ? toolRiskApi.getEvents(limit, offset)
        : toolRiskApi.getEvents(limit, offset, verdict),
  });

  const clearMutation = useMutation({
    mutationKey: ["toolRisk", "clearEvents"],
    mutationFn: (beforeTimestamp?: number) =>
      toolRiskApi.clearEvents(beforeTimestamp),
    onSuccess: () => {
      void invalidateSecurityEventDomains(queryClient);
      toast.success(
        t("toolRisk.eventsCleared", { defaultValue: "调用记录已清空" }),
      );
    },
    onError: () => toast.error(t("toolRisk.eventsClearFailed")),
  });

  return {
    events: query.data ?? [],
    isLoading: query.isLoading,
    refetch: query.refetch,
    clearEvents: (beforeTimestamp?: number) =>
      clearMutation.mutateAsync(beforeTimestamp),
    isClearing: clearMutation.isPending,
  };
}

export function useToolRiskEventBridge(onRecorded?: () => void) {
  const queryClient = useQueryClient();
  const callbackRef = useRef(onRecorded);
  callbackRef.current = onRecorded;

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    const refresh = () => {
      try {
        void Promise.resolve(
          queryClient.invalidateQueries({ queryKey: toolRiskKeys.all }),
        ).catch(() => reportFrontendError("tool_risk_invalidate", "failed"));
      } catch {
        reportFrontendError("tool_risk_invalidate", "failed");
      }
      void invalidateSecurityEventsQueries(queryClient);
    };
    void (async () => {
      try {
        const off = await listen("tool-risk-recorded", () => {
          if (disposed) return;
          refresh();
          callbackRef.current?.();
        });
        if (disposed) off();
        else {
          unlisten = off;
          refresh();

          callbackRef.current?.();
        }
      } catch {
        if (!disposed) reportFrontendError("tool_risk_listen", "failed");
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}

export function useToolRiskSandbox() {
  const { t } = useTranslation();
  const mutation = useMutation({
    mutationFn: (input: SandboxTestInput) => toolRiskApi.testRuleMatch(input),
    onError: () => toast.error(t("toolRisk.sandboxFailed")),
  });
  return {
    test: mutation.mutateAsync,
    result: mutation.data,
    isTesting: mutation.isPending,
    reset: mutation.reset,
  };
}

export function commandListErrorKey(error: unknown): string {
  const code = error instanceof Error ? error.message : error;
  switch (code) {
    case "tool_risk_command_list_duplicate":
      return "toolRisk.commandLists.duplicate";
    case "tool_risk_command_list_conflict":
      return "toolRisk.commandLists.conflict";
    case "tool_risk_command_list_limit":
      return "toolRisk.commandLists.limit";
    default:
      return "toolRisk.commandLists.mutationFailed";
  }
}

export function useToolRiskCommandLists() {
  const { t } = useTranslation();
  const writeNotice = useWriteNotice();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: toolRiskKeys.commandLists(),
    queryFn: () => toolRiskApi.getCommandLists(),
  });
  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: toolRiskKeys.all });
  };
  const save = useMutation({
    mutationFn: (input: SaveCommandListInput) =>
      toolRiskApi.saveCommandList(input),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt, "toolRisk.commandLists.saved");
    },
    onError: (error) => {
      invalidate();
      const key = commandListErrorKey(error);
      toast.error(
        t(
          key === "toolRisk.commandLists.mutationFailed"
            ? "toolRisk.policyWriteFailed"
            : key,
        ),
      );
    },
  });
  const remove = useMutation({
    mutationFn: ({
      id,
      expectedRevision,
    }: {
      id: string;
      expectedRevision: number;
    }) => toolRiskApi.deleteCommandList(id, expectedRevision),
    onSuccess: (receipt) => {
      invalidate();
      writeNotice(receipt);
    },
    onError: () => {
      invalidate();
      toast.error(t("toolRisk.policyWriteFailed"));
    },
  });
  return {
    entries: query.data ?? [],
    isLoading: query.isLoading,
    isError: query.isError,
    refetch: query.refetch,
    saveEntry: async (input: SaveCommandListInput) =>
      (await save.mutateAsync(input)).value,
    deleteEntry: async (id: string, expectedRevision: number) =>
      (await remove.mutateAsync({ id, expectedRevision })).value,
    isSaving: save.isPending,
    isDeleting: remove.isPending,
  };
}

export function useToolRiskPolicyChanges(limit = 50, offset = 0) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: toolRiskKeys.policyChanges(limit, offset),
    queryFn: () => toolRiskApi.getPolicyChanges(limit, offset),
  });
  const clear = useMutation({
    mutationFn: () => toolRiskApi.clearPolicyChanges(),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: [...toolRiskKeys.all, "policyChanges"],
      });
      toast.success(t("toolRisk.policyChanges.cleared"));
    },
    onError: () => toast.error(t("toolRisk.policyChanges.clearFailed")),
  });
  return {
    changes: query.data ?? [],
    isLoading: query.isLoading,
    isError: query.isError,
    isFetching: query.isFetching,
    refetch: query.refetch,
    clearChanges: clear.mutateAsync,
    isClearing: clear.isPending,
  };
}
