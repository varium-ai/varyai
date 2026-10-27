import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { getAppLabel } from "@/config/appConfig";
import type { SecurityEvent } from "@/types/securityEvents";
import {
  OPERATION_TYPES,
  type CommandListType,
  type ToolRiskRuleSeed,
} from "@/types/toolRisk";
import { ShellCommandListDialog } from "./ShellCommandListsPanel";
import { toolRiskRuleSeed } from "./toolRiskEventActions";
import {
  securityEventName,
  securityEventOutboundLabel,
  securityEventResultLabel,
  securityEventRiskLabel,
  toolCategoryLabel,
  toolReasonLabel,
  toolSeverityLabel,
  toolVerdictLabel,
} from "./securityEventLabels";

export function SecurityEventDetailsDialog({
  event,
  onClose,
  onCreateRule,
  onOpenDlpRules,
}: {
  event: SecurityEvent | null;
  onClose: () => void;
  onCreateRule?: (seed: ToolRiskRuleSeed) => void;
  onOpenDlpRules?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [listDraft, setListDraft] = useState<{
    listType: CommandListType;
    summary: string;
  } | null>(null);
  const field = (label: string, value: ReactNode) => (
    <div className="rounded-xl border border-border/70 bg-muted/30 p-3 text-xs">
      <div className="text-muted-foreground">{t(label)}</div>
      <div className="mt-0.5 break-all font-medium text-foreground">
        {value}
      </div>
    </div>
  );
  const unknown = t("securityEvents.unknown");
  const tool = event?.eventType === "tool_risk" ? event.record : null;
  const knownLabel = (value: string, labels: Record<string, string>) =>
    t(
      Object.prototype.hasOwnProperty.call(labels, value)
        ? labels[value]
        : "securityEvents.unknown",
    );
  return (
    <>
      <Dialog
        open={event !== null}
        onOpenChange={(open) => {
          if (!open) {
            setListDraft(null);
            onClose();
          }
        }}
      >
        <DialogContent zIndex="top" className="max-w-xl overflow-hidden p-0">
          <DialogHeader className="border-b border-border/70 px-6 py-4">
            <DialogTitle>{t("security.detailTitle")}</DialogTitle>
            <DialogDescription>
              {t(tool ? "securityEvents.toolWarning" : "security.logsDesc")}
            </DialogDescription>
          </DialogHeader>
          {event && (
            <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-5">
              <div className="grid grid-cols-2 gap-3">
                {field(
                  "securityEvents.source",
                  t(
                    event.eventType === "dlp"
                      ? "securityEvents.dlp"
                      : "securityEvents.toolRisk",
                  ),
                )}
                {field("security.detailType", securityEventName(t, event))}
                {field(
                  "security.detailService",
                  event.appType
                    ? getAppLabel(event.appType)
                    : t("securityEvents.notRecorded"),
                )}
                {field(
                  "security.riskLevel",
                  event.eventType === "tool_risk"
                    ? toolSeverityLabel(t, event.record.severity)
                    : securityEventRiskLabel(t, event.record.riskLevel, true),
                )}
                {field(
                  "security.recordResult",
                  securityEventResultLabel(t, event),
                )}
                {field(
                  "security.detailOutbound",
                  securityEventOutboundLabel(t, event),
                )}
                {field(
                  "security.firstSeenAt",
                  new Date(event.timestamp).toLocaleString(i18n.language),
                )}
                {field(
                  "security.lastSeenAt",
                  new Date(event.lastSeenAt).toLocaleString(i18n.language),
                )}
                {field(
                  "security.triggerCount",
                  event.triggerCount.toLocaleString(i18n.language),
                )}
              </div>
              {field(
                "security.sessionId",
                event.sessionId ||
                  t(
                    event.eventType === "dlp"
                      ? "security.sessionUnknown"
                      : "securityEvents.notRecorded",
                  ),
              )}
              {event.eventType === "dlp" ? (
                <div className="space-y-1.5">
                  <div className="text-xs text-muted-foreground">
                    {t("security.detailSnippet")}
                  </div>
                  <div className="max-h-32 overflow-y-auto break-all rounded-xl border bg-muted p-3.5 font-mono text-xs">
                    {event.record.matchedSnippetMasked}
                  </div>
                </div>
              ) : (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    {field(
                      "securityEvents.sourceStage",
                      <>
                        {knownLabel(event.record.source, {
                          proxy: "securityEvents.sourceProxy",
                          hook: "securityEvents.sourceHook",
                          client_hook: "securityEvents.sourceHook",
                          mcp_rpc: "securityEvents.sourceMcpRpc",
                          model_response: "securityEvents.sourceModelResponse",
                        })}{" "}
                        /{" "}
                        {knownLabel(event.record.stage, {
                          model_response: "securityEvents.stageModelResponse",
                          tool_request: "securityEvents.stageToolRequest",
                          response: "securityEvents.stageResponse",
                          proposed: "securityEvents.stageProposed",
                          pre_execution: "securityEvents.stagePreExecution",
                          rpc_submitted: "securityEvents.stageRpcSubmitted",
                        })}
                      </>,
                    )}
                    {field(
                      "securityEvents.integrity",
                      knownLabel(event.record.integrityStatus, {
                        complete: "securityEvents.integrityComplete",
                        incomplete: "securityEvents.integrityIncomplete",
                        invalid: "securityEvents.integrityInvalid",
                        unsupported: "securityEvents.integrityUnsupported",
                      }),
                    )}
                    {field(
                      "securityEvents.detectionScope",
                      knownLabel(event.record.detectionScope, {
                        in_scope: "securityEvents.scopeInScope",
                        out_of_scope: "securityEvents.scopeOutOfScope",
                        undetermined: "securityEvents.scopeUndetermined",
                      }),
                    )}
                    {field(
                      "toolRisk.colOperation",
                      OPERATION_TYPES.includes(
                        event.record.operationCategory as never,
                      )
                        ? t(
                            `toolRisk.operation.${event.record.operationCategory}`,
                          )
                        : unknown,
                    )}
                    {field(
                      "toolRisk.colVerdict",
                      toolVerdictLabel(t, event.record.verdict),
                    )}
                    {field(
                      "toolRisk.colCategory",
                      toolCategoryLabel(t, event.record.riskCategory),
                    )}
                    {field(
                      "toolRisk.colReason",
                      toolReasonLabel(t, event.record.reasonCode),
                    )}
                    {field(
                      "securityEvents.ruleIds",
                      event.record.ruleIds.length
                        ? event.record.ruleIds.join(", ")
                        : t("securityEvents.notRecorded"),
                    )}
                  </div>
                  <div className="space-y-2">
                    <div className="text-xs text-muted-foreground">
                      {t("toolRisk.colCommandSummary")}
                    </div>
                    <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all rounded-xl border bg-muted p-3 text-xs">
                      {event.record.commandSummary?.trim()
                        ? event.record.commandSummary
                        : t("toolRisk.commandSummaryUnavailable")}
                    </pre>
                    <p className="text-xs text-muted-foreground">
                      {t("securityEvents.commandPrivacy")}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {(["blacklist", "whitelist"] as const).map((listType) => (
                      <Button
                        key={listType}
                        variant="outline"
                        size="sm"
                        disabled={!event.record.commandSummary?.trim()}
                        onClick={() =>
                          setListDraft({
                            listType,
                            summary: event.record.commandSummary!,
                          })
                        }
                      >
                        {t(`toolRisk.commandLists.addTo.${listType}`)}
                      </Button>
                    ))}
                    {onCreateRule && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() =>
                          onCreateRule(toolRiskRuleSeed(event.record))
                        }
                      >
                        {t("toolRisk.createRuleFromEvent")}
                      </Button>
                    )}
                  </div>
                </>
              )}
            </div>
          )}
          <DialogFooter className="border-t border-border/70 px-6 py-4">
            {event?.eventType === "dlp" && onOpenDlpRules && (
              <Button variant="ghost" onClick={onOpenDlpRules}>
                {t("security.goToRules")}
              </Button>
            )}
            <Button
              onClick={() => {
                setListDraft(null);
                onClose();
              }}
            >
              {t("common.done")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {listDraft && (
        <ShellCommandListDialog
          open
          initialListType={listDraft.listType}
          commandSummary={listDraft.summary}
          onOpenChange={(open) => {
            if (!open) setListDraft(null);
          }}
        />
      )}
    </>
  );
}
