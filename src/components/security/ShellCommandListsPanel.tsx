import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  commandListErrorKey,
  useToolRiskCommandLists,
} from "@/hooks/useToolRisk";
import { toolRiskApi } from "@/lib/api";
import {
  RISK_CATEGORIES,
  RISK_SEVERITIES,
  type CommandListEntry,
  type CommandListType,
  type CommandMatchKind,
  type CommandPatternPreview,
  type SaveCommandListInput,
  type ToolRiskCategory,
} from "@/types/toolRisk";

const key = (name: string) => `toolRisk.commandLists.${name}`;
const summaryMarkers = /\[REDACTED\]|<redacted>|\.{3}|…|<path>|<url>|<secret>/i;
const simpleKinds: CommandMatchKind[] = ["prefix", "program", "exact"];
const advancedKinds: CommandMatchKind[] = ["keyword", "regex"];

function suggestPrefix(summary?: string | null): string {
  const words = summary?.trim().split(/\s+/) ?? [];
  if (!/^[\w./-]+$/.test(words[0] ?? "")) return "";
  return /^[a-z][\w-]*$/i.test(words[1] ?? "")
    ? words.slice(0, 2).join(" ")
    : words[0];
}
function toInput(entry: CommandListEntry): SaveCommandListInput {
  const { revision, ...fields } = entry;
  return { ...fields, expectedRevision: revision };
}

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  entry?: CommandListEntry;
  initialListType?: CommandListType;
  commandSummary?: string | null;
}
export function ShellCommandListDialog(props: DialogProps) {
  const { t } = useTranslation();
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent
        zIndex="top"
        className="max-h-[85vh] w-[calc(100%-2rem)] overflow-hidden sm:max-w-xl"
      >
        <DialogHeader>
          <DialogTitle>{t(key(props.entry ? "edit" : "add"))}</DialogTitle>
          <DialogDescription>{t(key("description"))}</DialogDescription>
        </DialogHeader>
        {props.open && <CommandListEditor {...props} />}
      </DialogContent>
    </Dialog>
  );
}

function CommandListEditor({
  entry,
  initialListType = "whitelist",
  commandSummary,
  onOpenChange,
}: DialogProps) {
  const { t } = useTranslation();
  const id = useId();
  const { entries, isLoading, isError, saveEntry, isSaving } =
    useToolRiskCommandLists();
  const [draft, setDraft] = useState<SaveCommandListInput>(() =>
    entry
      ? toInput(entry)
      : {
          listType: initialListType,
          matchKind: "prefix",
          pattern: suggestPrefix(commandSummary),
          enabled: true,
          severity: initialListType === "whitelist" ? "none" : "medium",
          riskCategory: "remote_code_execution",
        },
  );
  const [confirmed, setConfirmed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<CommandPatternPreview[] | null>(null);
  const [isPreviewing, setIsPreviewing] = useState(false);
  const [validated, setValidated] = useState("");
  const request = useRef(0);
  useEffect(
    () => () => {
      request.current += 1;
    },
    [],
  );
  const pattern = draft.pattern.trim();
  const signature = JSON.stringify([draft.matchKind, pattern]);
  const auditSummary = commandSummary != null;
  const invalidExact =
    draft.matchKind === "exact" && summaryMarkers.test(pattern);
  const invalidProgram = draft.matchKind === "program" && /\s/.test(pattern);

  const conflict = entries.some(
    (item) =>
      item.id !== draft.id &&
      item.enabled &&
      item.listType !== draft.listType &&
      (item.pattern.trim() === pattern ||
        (["program", "prefix"].includes(item.matchKind) &&
          ["program", "prefix"].includes(draft.matchKind) &&
          (pattern.startsWith(`${item.pattern.trim()} `) ||
            item.pattern.trim().startsWith(`${pattern} `))) ||
        advancedKinds.includes(item.matchKind) ||
        advancedKinds.includes(draft.matchKind)),
  );
  const canSave =
    !!pattern &&
    confirmed &&
    !isLoading &&
    !isError &&
    !isSaving &&
    !isPreviewing &&
    !invalidExact &&
    !invalidProgram &&
    (draft.matchKind !== "regex" || validated === signature);
  const patch = (changes: Partial<SaveCommandListInput>) => {
    setDraft((current) => ({ ...current, ...changes }));
    setConfirmed(false);
    setError(null);
    setPreview(null);
    setValidated("");
    setIsPreviewing(false);
    request.current += 1;
  };
  const runPreview = async () => {
    const current = ++request.current;
    setIsPreviewing(true);
    setError(null);
    setValidated("");
    setPreview(null);
    try {
      const result = await toolRiskApi.previewCommandPattern(
        draft.matchKind,
        pattern,
      );
      if (request.current !== current) return;
      setPreview(result.slice(0, 20));
      setValidated(signature);
    } catch {
      if (request.current === current) setError("previewFailed");
    } finally {
      if (request.current === current) setIsPreviewing(false);
    }
  };
  const save = async () => {
    if (!canSave) return;
    try {
      await saveEntry({
        ...draft,
        pattern,
        severity:
          draft.listType === "whitelist"
            ? "none"
            : draft.severity === "high"
              ? "high"
              : "medium",
      });
      onOpenChange(false);
    } catch (error) {
      setError(
        commandListErrorKey(error).split(".").at(-1) ?? "mutationFailed",
      );
    }
  };
  return (
    <>
      <div
        role="region"
        aria-label={t(key(entry ? "edit" : "add"))}
        className="min-h-0 flex-1 space-y-4 overflow-y-auto overscroll-contain px-6 py-4"
      >
        {auditSummary && (
          <div className="rounded-lg border border-border bg-muted/30 p-3 text-xs">
            <p className="font-semibold">{t("toolRisk.colCommandSummary")}</p>
            <p className="mt-1 break-all font-mono">{commandSummary}</p>
            <p className="mt-2">{t(key("summaryHint"))}</p>
            <p className="mt-1">{t(key("suggestionHint"))}</p>
          </div>
        )}
        <div className="space-y-1.5">
          <Label htmlFor={`${id}-type`}>{t(key("listType"))}</Label>
          <Select
            value={draft.listType}
            onValueChange={(value) =>
              patch({
                listType: value as CommandListType,
                severity: value === "whitelist" ? "none" : "medium",
              })
            }
          >
            <SelectTrigger id={`${id}-type`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="whitelist">{t(key("whitelist"))}</SelectItem>
              <SelectItem value="blacklist">{t(key("blacklist"))}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`${id}-kind`}>{t(key("matchKind"))}</Label>
          <Select
            value={draft.matchKind}
            onValueChange={(value) => {
              const matchKind = value as CommandMatchKind;
              patch({
                matchKind,
                ...(auditSummary && matchKind === "exact"
                  ? { pattern: "" }
                  : {}),
                ...(matchKind === "program"
                  ? { pattern: pattern.split(/\s+/)[0] }
                  : {}),
              });
            }}
          >
            <SelectTrigger id={`${id}-kind`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {[...simpleKinds, ...advancedKinds].map((kind) => (
                <SelectItem key={kind} value={kind}>
                  {t(key(`kind.${kind}`))}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`${id}-pattern`}>{t(key("pattern"))}</Label>
          <Input
            id={`${id}-pattern`}
            placeholder={t(key(`patternPlaceholder.${draft.matchKind}`))}
            value={draft.pattern}
            onChange={(e) => patch({ pattern: e.target.value })}
            aria-invalid={invalidExact || invalidProgram}
          />
          {invalidExact && (
            <p role="alert" className="text-xs text-destructive">
              {t(key("exactSummaryBlocked"))}
            </p>
          )}
          {invalidProgram && (
            <p role="alert" className="text-xs text-destructive">
              {t(key("programInvalid"))}
            </p>
          )}
        </div>
        <div
          className="rounded-lg border border-border bg-muted/30 p-3 text-xs"
          data-testid="command-list-scope"
        >
          <p>{t(key(`scope.${draft.matchKind}`))}</p>
          {draft.matchKind === "program" && (
            <p className="mt-2">
              {t(
                key(
                  draft.listType === "blacklist"
                    ? "blacklistProgramHint"
                    : "whitelistProgramHint",
                ),
              )}
            </p>
          )}
          {draft.listType === "whitelist" && (
            <>
              <p className="mt-2">{t(key("whitelistHint"))}</p>
              <p className="mt-2">
                {t(
                  key(
                    draft.matchKind === "program" ||
                      draft.matchKind === "prefix"
                      ? "segmentedWhitelistHint"
                      : "singleSegmentWhitelistHint",
                  ),
                )}
              </p>
            </>
          )}
          {conflict && pattern && (
            <p role="alert" className="mt-2 text-amber-600">
              {t(key("conflictHint"))}
            </p>
          )}
        </div>
        {draft.listType === "blacklist" && (
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor={`${id}-severity`}>{t(key("severity"))}</Label>
              <Select
                value={draft.severity}
                onValueChange={(value) =>
                  patch({ severity: value as "medium" | "high" })
                }
              >
                <SelectTrigger id={`${id}-severity`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RISK_SEVERITIES.map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`toolRisk.severity.${s}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor={`${id}-category`}>{t(key("riskCategory"))}</Label>
              <Select
                value={draft.riskCategory}
                onValueChange={(value) =>
                  patch({ riskCategory: value as ToolRiskCategory })
                }
              >
                <SelectTrigger id={`${id}-category`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RISK_CATEGORIES.map((category) => (
                    <SelectItem key={category} value={category}>
                      {t(`toolRisk.category.${category}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}
        <Button
          size="sm"
          variant="outline"
          disabled={!pattern || isPreviewing}
          onClick={() => void runPreview()}
        >
          {t(key("preview"))}
        </Button>
        <p className="text-xs text-muted-foreground">{t(key("previewHint"))}</p>
        {draft.matchKind === "regex" && validated !== signature && (
          <p className="text-xs text-muted-foreground">
            {t(key("regexValidationRequired"))}
          </p>
        )}
        {preview && (
          <div className="space-y-2 text-xs">
            {preview.length === 0 && <p>{t(key("noPreview"))}</p>}
            {preview.map((row) => (
              <div
                key={row.eventId}
                className="flex flex-wrap items-center gap-2"
              >
                <span>
                  {t(key("eventId"))}: {row.eventId}
                </span>
                <Badge variant={row.matched ? "secondary" : "outline"}>
                  {t(key(row.matched ? "matched" : "notMatched"))}
                </Badge>
                {row.summaryOnly && (
                  <Badge variant="outline">{t(key("summaryOnly"))}</Badge>
                )}
              </div>
            ))}
          </div>
        )}
        <label className="flex items-start gap-2 text-xs">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />
          {t(key("confirmScope"))}
        </label>
        {(error || isError) && (
          <p role="alert" className="text-xs text-destructive">
            {t(key(error ?? "loadFailed"))}
          </p>
        )}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          {t("common.cancel")}
        </Button>
        <Button disabled={!canSave} onClick={() => void save()}>
          {t(key("confirmSave"))}
        </Button>
      </DialogFooter>
    </>
  );
}

export function ShellCommandListsPanel() {
  const { t } = useTranslation();
  const {
    entries,
    isLoading,
    isError,
    refetch,
    saveEntry,
    deleteEntry,
    isSaving,
    isDeleting,
  } = useToolRiskCommandLists();
  const [dialog, setDialog] = useState<{ entry?: CommandListEntry } | null>(
    null,
  );
  const [deleting, setDeleting] = useState<CommandListEntry | null>(null);
  const [error, setError] = useState(false);
  const toggle = async (entry: CommandListEntry, enabled: boolean) => {
    setError(false);
    try {
      await saveEntry({ ...toInput(entry), enabled });
    } catch {
      setError(true);
    }
  };
  const remove = async () => {
    if (!deleting) return;
    setError(false);
    try {
      await deleteEntry(deleting.id, deleting.revision);
      setDeleting(null);
    } catch {
      setError(true);
    }
  };
  return (
    <section
      data-testid="shell-command-lists-panel"
      className="rounded-2xl border border-border/90 bg-card shadow-sm"
    >
      <div className="flex items-center justify-between gap-3 border-b border-border/80 px-5 py-4">
        <div>
          <h3 className="text-sm font-semibold">{t(key("title"))}</h3>
          <p className="mt-1 text-xs text-muted-foreground">
            {t(key("description"))}
          </p>
        </div>
        <Button size="sm" onClick={() => setDialog({})}>
          {t(key("add"))}
        </Button>
      </div>
      {(error || isError) && (
        <div role="alert" className="px-5 py-3 text-xs text-destructive">
          {t(key(isError ? "loadFailed" : "mutationFailed"))}
          <Button size="sm" variant="ghost" onClick={() => void refetch()}>
            {t("common.refresh")}
          </Button>
        </div>
      )}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t(key("listType"))}</TableHead>
            <TableHead>{t(key("pattern"))}</TableHead>
            <TableHead>{t(key("matchKind"))}</TableHead>
            <TableHead>{t(key("severity"))}</TableHead>
            <TableHead>{t("toolRisk.colActions")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {isLoading ? (
            <TableRow>
              <TableCell colSpan={5}>{t("common.loading")}</TableCell>
            </TableRow>
          ) : entries.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={5}
                className="py-6 text-center text-xs text-muted-foreground"
              >
                {t(key("empty"))}
              </TableCell>
            </TableRow>
          ) : (
            entries.map((entry) => (
              <TableRow key={entry.id}>
                <TableCell className="text-xs">
                  {t(key(entry.listType))}
                </TableCell>
                <TableCell className="max-w-[240px] break-all font-mono text-xs">
                  {entry.pattern}
                </TableCell>
                <TableCell className="text-xs">
                  {t(key(`kind.${entry.matchKind}`))}
                </TableCell>
                <TableCell className="text-xs">
                  {t(`toolRisk.severity.${entry.severity}`)}
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <Switch
                      aria-label={t(key("enabled"))}
                      checked={entry.enabled}
                      disabled={isSaving || isDeleting}
                      onCheckedChange={(checked) => void toggle(entry, checked)}
                    />
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => setDialog({ entry })}
                    >
                      {t("common.edit")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={isSaving || isDeleting}
                      onClick={() => {
                        setError(false);
                        setDeleting(entry);
                      }}
                    >
                      {t("common.delete")}
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
      <ShellCommandListDialog
        open={dialog != null}
        entry={dialog?.entry}
        onOpenChange={(open) => !open && setDialog(null)}
      />
      <Dialog
        open={deleting != null}
        onOpenChange={(open) => !open && setDeleting(null)}
      >
        <DialogContent zIndex="top">
          <DialogHeader>
            <DialogTitle>{t(key("confirmDelete"))}</DialogTitle>
            <DialogDescription>{t(key("deleteHint"))}</DialogDescription>
          </DialogHeader>
          {error && (
            <p role="alert" className="px-6 text-xs text-destructive">
              {t(key("mutationFailed"))}
            </p>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleting(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={isDeleting}
              onClick={() => void remove()}
            >
              {t("common.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
