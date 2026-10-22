import { invoke } from "@tauri-apps/api/core";
import { projectToolRiskPolicyChanges } from "@/types/toolRisk";
import type {
  CommandListEntry,
  CommandMatchKind,
  CommandPatternPreview,
  SaveCommandListInput,
  SaveCustomRuleInput,
  SandboxTestInput,
  SandboxTestResult,
  ToolRiskEventRecord,
  ToolRiskRule,
  ToolRiskRuleItem,
  ToolRiskSettings,
  ToolRiskVerdict,
  ToolRiskWriteReceipt,
  ToolRiskPolicyChangeRecord,
} from "@/types/toolRisk";

export const toolRiskApi = {
  async getCommandLists(): Promise<CommandListEntry[]> {
    return invoke("get_tool_risk_command_lists");
  },
  async saveCommandList(
    input: SaveCommandListInput,
  ): Promise<ToolRiskWriteReceipt<string>> {
    return invoke("save_tool_risk_command_list", { input });
  },
  async deleteCommandList(
    id: string,
    expectedRevision: number,
  ): Promise<ToolRiskWriteReceipt<boolean>> {
    return invoke("delete_tool_risk_command_list", { id, expectedRevision });
  },
  async previewCommandPattern(
    matchKind: CommandMatchKind,
    pattern: string,
  ): Promise<CommandPatternPreview[]> {
    return invoke("preview_tool_risk_command_pattern", { matchKind, pattern });
  },

  async getRules(): Promise<ToolRiskRuleItem[]> {
    return invoke("get_tool_risk_rules");
  },

  async getRule(id: string): Promise<ToolRiskRule | null> {
    return invoke("get_tool_risk_rule", { id });
  },

  async saveCustomRule(
    input: SaveCustomRuleInput,
  ): Promise<ToolRiskWriteReceipt<string>> {
    return invoke("save_custom_tool_risk_rule", { input });
  },

  async deleteCustomRule(
    id: string,
    expectedRevision: number,
  ): Promise<ToolRiskWriteReceipt<boolean>> {
    return invoke("delete_custom_tool_risk_rule", { id, expectedRevision });
  },

  async toggleRule(
    id: string,
    enabled: boolean,
    expectedRevision: number,
  ): Promise<ToolRiskWriteReceipt<null>> {
    return invoke("toggle_tool_risk_rule", { id, enabled, expectedRevision });
  },

  async resetBuiltinRule(
    id: string,
    expectedRevision: number,
  ): Promise<ToolRiskWriteReceipt<null>> {
    return invoke("reset_builtin_tool_risk_rule", { id, expectedRevision });
  },

  async testRuleMatch(input: SandboxTestInput): Promise<SandboxTestResult> {
    return invoke("test_tool_risk_rule_match", { input });
  },

  async getSettings(): Promise<ToolRiskSettings> {
    return invoke("get_tool_risk_settings");
  },

  async updateSettings(
    settings: ToolRiskSettings,
  ): Promise<ToolRiskWriteReceipt<null>> {
    return invoke("update_tool_risk_settings", { settings });
  },

  async getPolicyChanges(
    limit: number,
    offset: number,
  ): Promise<ToolRiskPolicyChangeRecord[]> {
    const records = await invoke<unknown>("get_tool_risk_policy_changes", {
      limit,
      offset,
    });
    return projectToolRiskPolicyChanges(records);
  },

  async clearPolicyChanges(): Promise<number> {
    return invoke("clear_tool_risk_policy_changes");
  },

  async getEvents(
    limit: number,
    offset: number,
    verdict?: ToolRiskVerdict,
  ): Promise<ToolRiskEventRecord[]> {
    return invoke("get_tool_risk_events", {
      limit,
      offset,
      ...(verdict === undefined ? {} : { verdict }),
    });
  },

  async getEventsAfter(
    afterId: number,
    limit: number,
    policyVersion: number,
  ): Promise<ToolRiskEventRecord[]> {
    return invoke("get_tool_risk_events_after", {
      afterId,
      limit,
      policyVersion,
    });
  },

  async clearEvents(beforeTimestamp?: number): Promise<number> {
    return invoke("clear_tool_risk_events", { beforeTimestamp });
  },
};
