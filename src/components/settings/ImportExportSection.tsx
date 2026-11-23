import { useMemo } from "react";
import {
  AlertCircle,
  CheckCircle2,
  FolderOpen,
  Loader2,
  Save,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { ImportStatus } from "@/hooks/useImportExport";

interface ImportExportSectionProps {
  status: ImportStatus;
  selectedFile: string;
  errorMessage: string | null;
  backupId: string | null;
  isImporting: boolean;
  onSelectFile: () => Promise<void>;
  onImport: () => Promise<void>;
  onExport: () => Promise<void>;
  onClear: () => void;
}

export function ImportExportSection({
  status,
  selectedFile,
  errorMessage,
  backupId,
  isImporting,
  onSelectFile,
  onImport,
  onExport,
  onClear,
}: ImportExportSectionProps) {
  const { t } = useTranslation();

  const selectedFileName = useMemo(() => {
    if (!selectedFile) return "";
    const segments = selectedFile.split(/[\\/]/);
    return segments[segments.length - 1] || selectedFile;
  }, [selectedFile]);

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium text-foreground">
          {t("settings.importExport")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.importExportHint")}
        </p>
      </header>

      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant={selectedFile ? "default" : "outline"}
            onClick={!selectedFile ? onSelectFile : onImport}
            disabled={isImporting}
          >
            {isImporting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : selectedFile ? (
              <CheckCircle2 className="h-4 w-4" />
            ) : (
              <FolderOpen className="h-4 w-4" />
            )}
            {isImporting
              ? t("settings.importing")
              : selectedFile
                ? t("settings.import")
                : t("settings.selectConfigFile")}
          </Button>
          <Button type="button" variant="outline" onClick={onExport}>
            <Save className="h-4 w-4" />
            {t("settings.exportConfig")}
          </Button>
        </div>
        {selectedFile && (
          <div className="flex max-w-full items-center gap-2 rounded-md border border-border bg-muted px-3 py-1">
            <p
              className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground"
              title={selectedFileName}
            >
              {selectedFileName}
            </p>
            <Button
              type="button"
              size="icon"
              variant="ghost"
              onClick={onClear}
              aria-label={t("common.clear")}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        )}

        <ImportStatusMessage
          status={status}
          errorMessage={errorMessage}
          backupId={backupId}
        />
      </div>
    </section>
  );
}

interface ImportStatusMessageProps {
  status: ImportStatus;
  errorMessage: string | null;
  backupId: string | null;
}

function ImportStatusMessage({
  status,
  errorMessage,
  backupId,
}: ImportStatusMessageProps) {
  const { t } = useTranslation();

  if (status === "idle") {
    return null;
  }

  const baseClass =
    "flex items-start gap-3 rounded-md border p-3 text-sm leading-5";

  if (status === "importing") {
    return (
      <div className={`${baseClass} border-border bg-accent text-primary`}>
        <Loader2 className="mt-0.5 h-5 w-5 flex-shrink-0 animate-spin" />
        <div>
          <p className="font-semibold">{t("settings.importing")}</p>
          <p className="text-xs">{t("common.loading")}</p>
        </div>
      </div>
    );
  }

  if (status === "success") {
    return (
      <div
        className={`${baseClass} border-border bg-[var(--success-surface)] text-[var(--success)]`}
      >
        <CheckCircle2 className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div className="space-y-1.5">
          <p className="font-semibold">{t("settings.importSuccess")}</p>
          {backupId ? (
            <p className="text-xs">
              {t("settings.backupId")}: {backupId}
            </p>
          ) : null}
          <p className="text-xs">{t("settings.autoReload")}</p>
        </div>
      </div>
    );
  }

  if (status === "partial-success") {
    return (
      <div
        className={`${baseClass} border-border bg-[var(--warning-surface)] text-[var(--warning)]`}
      >
        <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div className="space-y-1.5">
          <p className="font-semibold">{t("settings.importPartialSuccess")}</p>
          <p className="text-xs">{t("settings.importPartialHint")}</p>
        </div>
      </div>
    );
  }

  const message = errorMessage || t("settings.importFailed");

  return (
    <div
      className={`${baseClass} border-border bg-[var(--danger-surface)] text-destructive`}
    >
      <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
      <div className="space-y-1.5">
        <p className="font-semibold">{t("settings.importFailed")}</p>
        <p className="text-xs">{message}</p>
      </div>
    </div>
  );
}
