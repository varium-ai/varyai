import { resolveUsageRange } from "@/lib/usageRange";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { securityApi } from "@/lib/api";
import type {
  SecurityAuditSettings,
  CustomSecurityRule,
  AuditAction,
  AuditLogFilter,
} from "@/types/security";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

export const securityKeys = {
  all: ["security"] as const,
  settings: () => [...securityKeys.all, "settings"] as const,
  rules: () => [...securityKeys.all, "rules"] as const,
  logs: (filter: AuditLogFilter, page: number, pageSize: number) =>
    [...securityKeys.all, "logs", filter, page, pageSize] as const,
};

export function useSecuritySettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: securityKeys.settings(),
    queryFn: () => securityApi.getSettings(),
  });

  const mutation = useMutation({
    mutationFn: (newSettings: SecurityAuditSettings) =>
      securityApi.updateSettings(newSettings),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.settings() });
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
      toast.success(
        t("security.settingsUpdated", { defaultValue: "安全审计策略已更新" }),
      );
    },
    onError: (err: any) => {
      toast.error(
        t("security.settingsUpdateFailed", {
          defaultValue: "更新安全策略失败: ",
        }) + String(err),
      );
    },
  });

  return {
    settings: query.data,
    isLoading: query.isLoading,
    updateSettings: mutation.mutateAsync,
    isUpdating: mutation.isPending,
  };
}

export function useSecurityRules() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: securityKeys.rules(),
    queryFn: () => securityApi.getRules(),
  });

  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      securityApi.toggleCustomRule(id, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
    },
    onError: (err: any) => {
      toast.error(String(err));
    },
  });

  const builtinActionMutation = useMutation({
    mutationFn: ({ id, action }: { id: string; action: AuditAction | null }) =>
      securityApi.updateBuiltinRuleAction(id, action),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
    },
    onError: (err: any) => {
      toast.error(String(err));
    },
  });

  const customActionMutation = useMutation({
    mutationFn: ({ id, action }: { id: string; action: AuditAction }) =>
      securityApi.updateCustomRuleAction(id, action),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
    },
    onError: (err: any) => {
      toast.error(String(err));
    },
  });

  const saveMutation = useMutation({
    mutationFn: (rule: CustomSecurityRule) => securityApi.saveCustomRule(rule),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
      toast.success(
        t("security.ruleSaved", { defaultValue: "自定义规则已保存" }),
      );
    },
    onError: (err: any) => {
      toast.error(
        t("security.ruleSaveFailed", { defaultValue: "保存规则失败: " }) +
          String(err),
      );
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => securityApi.deleteCustomRule(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.rules() });
      toast.success(t("security.ruleDeleted", { defaultValue: "规则已删除" }));
    },
    onError: (err: any) => {
      toast.error(String(err));
    },
  });

  return {
    rules: query.data ?? [],
    isLoading: query.isLoading,
    refetch: query.refetch,
    toggleRule: (id: string, enabled: boolean) =>
      toggleMutation.mutateAsync({ id, enabled }),
    updateBuiltinAction: (id: string, action: AuditAction | null) =>
      builtinActionMutation.mutateAsync({ id, action }),
    updateCustomAction: (id: string, action: AuditAction) =>
      customActionMutation.mutateAsync({ id, action }),
    saveRule: saveMutation.mutateAsync,
    deleteRule: deleteMutation.mutateAsync,
    isSaving: saveMutation.isPending,
  };
}

export function useSecurityAuditLogs(
  filter: AuditLogFilter,
  page: number,
  pageSize: number,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: securityKeys.logs(filter, page, pageSize),
    queryFn: () => securityApi.getAuditLogs(filter, page, pageSize),
  });

  const clearMutation = useMutation({
    mutationFn: (beforeTimestamp?: number) =>
      securityApi.clearAuditLogs(beforeTimestamp),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.all });
      toast.success(
        t("security.logsCleared", { defaultValue: "安全事件已清空" }),
      );
    },
    onError: (err: any) => {
      toast.error(String(err));
    },
  });

  return {
    logsResult: query.data,
    isError: query.isError,
    isLoading: query.isLoading,
    refetch: query.refetch,
    clearLogs: (beforeTimestamp?: number) =>
      clearMutation.mutateAsync(beforeTimestamp),
    isClearing: clearMutation.isPending,
  };
}

export function useSecurityEventCount(appType: string) {
  return useQuery({
    queryKey: [...securityKeys.all, "event-count", appType],
    queryFn: () => {
      const { startDate } = resolveUsageRange({ preset: "today" });
      return securityApi.getEventCount(appType, startDate * 1000, Date.now());
    },
    refetchInterval: 30000,
    refetchIntervalInBackground: false,
  });
}
