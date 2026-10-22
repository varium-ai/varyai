import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Eraser, RefreshCw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
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
import { useToolRiskPolicyChanges } from "@/hooks/useToolRisk";
import { projectToolRiskPolicyChanges } from "@/types/toolRisk";

const PAGE_SIZE = 50;
const key = (name: string) => `toolRisk.policyChanges.${name}`;

export function ToolRiskPolicyChangesPanel({
  onClose,
}: {
  onClose?: () => void;
} = {}) {
  const { t } = useTranslation();
  const [offset, setOffset] = useState(0);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [clearFailed, setClearFailed] = useState(false);
  const {
    changes,
    isLoading,
    isError,
    isFetching,
    refetch,
    clearChanges,
    isClearing,
  } = useToolRiskPolicyChanges(PAGE_SIZE, offset);
  const rows = projectToolRiskPolicyChanges(changes);
  const busy = isFetching || isClearing;
  const clear = async () => {
    if (isClearing) return;
    setClearFailed(false);
    try {
      await clearChanges();
      setOffset(0);
      setConfirmOpen(false);
    } catch {
      setClearFailed(true);
    }
  };
  return (
    <section
      className={
        onClose
          ? "flex min-h-0 flex-1 flex-col bg-card"
          : "rounded-2xl border border-border/90 bg-card shadow-sm"
      }
      aria-label={t(key("title"))}
    >
      <div className="flex shrink-0 flex-wrap items-start justify-between gap-3 border-b border-border/80 px-5 py-4">
        <div className="min-w-0 flex-1">
          {onClose ? (
            <>
              <DialogTitle className="text-sm font-semibold">
                {t(key("title"))}
              </DialogTitle>
              <DialogDescription className="mt-1 text-xs">
                {t(key("description"))}
              </DialogDescription>
            </>
          ) : (
            <>
              <h3 className="text-sm font-semibold">{t(key("title"))}</h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {t(key("description"))}
              </p>
            </>
          )}
          <p className="mt-1 text-xs text-muted-foreground">
            {t(key("metadataOnly"))}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            {t(key("actorHint"))}
          </p>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => {
              void refetch();
            }}
          >
            <RefreshCw className="mr-1 h-4 w-4" />
            {t("common.refresh")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => {
              setClearFailed(false);
              setConfirmOpen(true);
            }}
          >
            <Eraser className="mr-1 h-4 w-4" />
            {t(key("clear"))}
          </Button>
          {onClose && (
            <Button
              size="icon"
              variant="ghost"
              className="h-8 w-8"
              aria-label={t("common.close")}
              onClick={onClose}
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {isLoading ? (
          <p className="p-8 text-center text-sm">{t("common.loading")}</p>
        ) : isError ? (
          <p role="alert" className="p-8 text-center text-sm text-destructive">
            {t(key("loadFailed"))}
          </p>
        ) : rows.length === 0 ? (
          <p className="p-8 text-center text-sm text-muted-foreground">
            {t(key("empty"))}
          </p>
        ) : (
          <Table className="min-w-[800px]">
            <TableHeader>
              <TableRow>
                {[
                  "colTime",
                  "colVersion",
                  "colActor",
                  "colKind",
                  "colTarget",
                  "colFields",
                  "colAudit",
                ].map((name) => (
                  <TableHead key={name}>{t(key(name))}</TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="text-xs">
                    {new Date(row.at).toLocaleString()}
                  </TableCell>
                  <TableCell className="text-xs">{row.policyVersion}</TableCell>
                  <TableCell className="text-xs">
                    {t(key(`actor.${row.actor}`))}
                  </TableCell>
                  <TableCell className="text-xs">
                    {t(key(`kind.${row.kind}`))}
                  </TableCell>
                  <TableCell className="break-all text-xs">
                    {row.targetId === null
                      ? t(key("noTarget"))
                      : row.targetId === "<unknown_target>"
                        ? t(key("targetUnknown"))
                        : row.targetId}
                  </TableCell>
                  <TableCell className="text-xs">
                    {row.changedFields.length === 0
                      ? t(key("noFields"))
                      : row.changedFields.map((field) => (
                          <div key={field}>{t(key(`field.${field}`))}</div>
                        ))}
                  </TableCell>
                  <TableCell className="text-xs">
                    {t(key(`audit.${row.auditStatus}`))}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </div>
      <div className="flex shrink-0 items-center justify-between border-t border-border/80 px-5 py-3">
        <span className="text-xs text-muted-foreground">
          {t(key("page"), { page: offset / PAGE_SIZE + 1 })}
        </span>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={offset === 0 || busy}
            onClick={() =>
              setOffset((previous) => Math.max(0, previous - PAGE_SIZE))
            }
          >
            {t("security.prevPage")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={isError || rows.length < PAGE_SIZE || busy}
            onClick={() => setOffset((previous) => previous + PAGE_SIZE)}
          >
            {t("security.nextPage")}
          </Button>
        </div>
      </div>
      <Dialog
        open={confirmOpen}
        onOpenChange={(open) => {
          if (!isClearing) setConfirmOpen(open);
        }}
      >
        <DialogContent
          zIndex={onClose ? "top" : "base"}
          className={onClose ? "z-[120]" : undefined}
          overlayClassName={onClose ? "z-[120]" : undefined}
        >
          <DialogHeader>
            <DialogTitle>{t(key("clearConfirmTitle"))}</DialogTitle>
            <DialogDescription>
              {t(key("clearConfirmDescription"))}
            </DialogDescription>
          </DialogHeader>
          {clearFailed && (
            <p role="alert" className="text-sm text-destructive">
              {t(key("clearFailed"))}
            </p>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={isClearing}
              onClick={() => setConfirmOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={isClearing}
              onClick={() => {
                void clear();
              }}
            >
              {t(key("confirmClear"))}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
