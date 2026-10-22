import { invoke } from "@tauri-apps/api/core";
import type {
  SecurityAuditSettings,
  AuditAction,
  SecurityRuleItem,
  CustomSecurityRule,
  AuditLogFilter,
  AuditLogPageResult,
  RuleMatchResult,
  AuditLogRecord,
} from "@/types/security";

export const securityApi = {
  async getEventCount(
    appType: string,
    startTimestamp: number,
    endTimestamp: number,
  ): Promise<number> {
    return invoke("get_security_event_count", {
      appType,
      startTimestamp,
      endTimestamp,
    });
  },

  async getSettings(): Promise<SecurityAuditSettings> {
    return invoke("get_security_audit_settings");
  },

  async updateSettings(settings: SecurityAuditSettings): Promise<void> {
    return invoke("update_security_audit_settings", { settings });
  },

  async getAuditLogs(
    filter: AuditLogFilter,
    page: number,
    pageSize: number,
  ): Promise<AuditLogPageResult> {
    return invoke("get_security_audit_logs", { filter, page, pageSize });
  },

  async getAuditNotificationBatch(
    sinceId: number,
    mountedAt: number,
    limit: number,
  ): Promise<AuditLogRecord[]> {
    return invoke("get_security_audit_notification_batch", {
      sinceId,
      mountedAt,
      limit,
    });
  },

  async clearAuditLogs(beforeTimestamp?: number): Promise<number> {
    return invoke("clear_security_audit_logs", { beforeTimestamp });
  },

  async getRules(): Promise<SecurityRuleItem[]> {
    return invoke("get_security_rules");
  },

  async updateBuiltinRuleAction(
    id: string,
    action: AuditAction | null,
  ): Promise<void> {
    return invoke("update_builtin_security_rule_action", { id, action });
  },

  async updateCustomRuleAction(id: string, action: AuditAction): Promise<void> {
    return invoke("update_custom_security_rule_action", { id, action });
  },

  async saveCustomRule(rule: CustomSecurityRule): Promise<void> {
    return invoke("save_custom_security_rule", { rule });
  },

  async toggleCustomRule(id: string, enabled: boolean): Promise<boolean> {
    return invoke("toggle_custom_security_rule", { id, enabled });
  },

  async deleteCustomRule(id: string): Promise<boolean> {
    return invoke("delete_custom_security_rule", { id });
  },

  async testRuleMatch(text: string): Promise<RuleMatchResult[]> {
    return invoke("test_security_rule_match", { text });
  },

  async getAuditAlerts(sinceTimestamp?: number): Promise<AuditLogRecord[]> {
    return invoke("get_security_audit_alerts", { sinceTimestamp });
  },
};
