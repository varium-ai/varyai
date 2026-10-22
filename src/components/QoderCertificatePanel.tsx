import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export function QoderCertificatePanel() {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const [action, setAction] = useState<"clear" | "rebuild" | null>(null);
  const [pending, setPending] = useState(false);
  const { data, isPending, isError } = useQuery({
    queryKey: ["qoderCertificateStatus"],
    queryFn: () =>
      invoke<{
        expires_at: number | null;
        expired: boolean;
        fingerprint: string | null;
        active: boolean;
      }>("get_qoder_certificate_status"),
    refetchInterval: 2000,
  });
  const run = async () => {
    if (!action || pending) return;
    setPending(true);
    try {
      await invoke(
        action === "clear"
          ? "clear_qoder_certificate"
          : "rebuild_qoder_certificate",
      );
      await client.invalidateQueries();
      toast.success(
        t(`qoder.certificate.${action === "clear" ? "cleared" : "renewed"}`),
      );
      setAction(null);
    } catch {
      toast.error(t("qoder.certificate.operationFailed"));
    } finally {
      setPending(false);
    }
  };
  return (
    <section
      className="rounded-2xl border border-slate-200 bg-white p-5 dark:border-border dark:bg-card"
      aria-label={t("qoder.certificate.manageTitle")}
    >
      <h2>
        <button
          type="button"
          aria-label={t("qoder.certificate.manageTitle")}
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full flex-wrap items-center justify-between gap-3 text-left"
        >
          <span className="font-semibold">
            {t("qoder.certificate.manageTitle")}
          </span>
          <span className="flex items-center gap-3">
            <span
              className="text-xs font-normal text-muted-foreground"
              role="status"
            >
              {isError ? (
                t("qoder.certificate.statusFailed")
              ) : isPending ? (
                t("common.loading")
              ) : data?.expires_at ? (
                <>
                  {t(
                    data.expired
                      ? "qoder.certificate.expiredTitle"
                      : "qoder.certificate.valid",
                  )}{" "}
                  ·{" "}
                  {t("qoder.certificate.expiresAt", {
                    date: new Date(data.expires_at * 1000).toLocaleString(
                      i18n.language,
                    ),
                  })}
                </>
              ) : (
                t("qoder.certificate.absent")
              )}
            </span>
            <ChevronDown
              aria-hidden="true"
              className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`}
            />
          </span>
        </button>
      </h2>
      <div
        id={detailsId}
        hidden={!expanded}
        className="mt-3 border-t border-border pt-3"
      >
        <p className="text-sm text-muted-foreground">
          {t("qoder.certificate.purpose")}
        </p>
        {data?.fingerprint && (
          <p className="mt-1 break-all text-xs text-muted-foreground">
            {t("qoder.certificate.fingerprint")}:{" "}
            <code>{data.fingerprint}</code>
          </p>
        )}
        <div className="mt-4 flex gap-2">
          <Button
            variant="outline"
            disabled={pending || isPending || isError}
            onClick={() => setAction("rebuild")}
          >
            {t("qoder.certificate.rebuild")}
          </Button>
          <Button
            variant="outline"
            disabled={pending || isPending || isError || !data?.expires_at}
            onClick={() => setAction("clear")}
          >
            {t("qoder.certificate.clear")}
          </Button>
        </div>
      </div>
      <ConfirmDialog
        isOpen={action !== null}
        title={t(
          `qoder.certificate.${action === "clear" ? "clear" : "rebuild"}`,
        )}
        message={t(
          `qoder.certificate.${action === "clear" ? "clearDescription" : "rebuildDescription"}`,
        )}
        confirmText={t("common.confirm")}
        variant={action === "clear" ? "destructive" : "info"}
        pending={pending}
        onConfirm={() => void run()}
        onCancel={() => {
          if (!pending) setAction(null);
        }}
      />
    </section>
  );
}
