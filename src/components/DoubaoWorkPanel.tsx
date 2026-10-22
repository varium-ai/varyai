import { useId, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";
import { toast } from "sonner";
import { proxyApi } from "@/lib/api/proxy";
import { useSecurityProtection } from "@/hooks/useSecurityProtection";
import { Button } from "@/components/ui/button";
import { extractErrorMessage } from "@/utils/errorUtils";

export function DoubaoWorkPanel() {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const details = useId();
  const protection = useSecurityProtection("doubaowork");
  const client = useQueryClient();
  const launch = useMutation({
    mutationFn: async () => {
      await protection.setAppEnabled(true);
      await proxyApi.launchDoubaoWorkForAudit();
    },
    onSuccess: () => toast.info(t("homeAudit.doubaoWaiting")),
    onError: (error) => {
      const key = extractErrorMessage(error);
      toast.error(
        t(key.startsWith("homeAudit.") ? key : "homeAudit.doubaoLaunchFailed"),
      );
    },
    onSettled: () => client.invalidateQueries(),
  });
  return (
    <section className="space-y-4 rounded-2xl border bg-card p-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="space-y-2">
          <h2 className="text-lg font-semibold">
            {t("homeAudit.doubaoAccount")}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t("homeAudit.doubaoScope")}
          </p>
        </div>
        {protection.connected ? (
          <Button
            variant="outline"
            disabled={protection.isUpdating}
            onClick={() => void protection.setAppEnabled(false).catch(() => {})}
          >
            {t("homeAudit.doubaoDisconnect")}
          </Button>
        ) : (
          <Button
            disabled={launch.isPending || protection.isUpdating}
            onClick={() => launch.mutate()}
          >
            {t("homeAudit.doubaoLaunch")}
          </Button>
        )}
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        {t("homeAudit.newConversationNotice")}
      </p>
      <div className="border-t pt-4">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={details}
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 text-sm font-medium"
        >
          <ChevronRight
            className={`h-4 w-4 transition-transform ${expanded ? "rotate-90" : ""}`}
          />
          {t("homeAudit.doubaoNoticeTitle")}
        </button>
        <div
          id={details}
          hidden={!expanded}
          className="mt-3 space-y-2 text-sm leading-relaxed text-muted-foreground"
        >
          <p>{t("homeAudit.doubaoConnection")}</p>
          <p>{t("homeAudit.doubaoRestore")}</p>
          <p>{t("homeAudit.doubaoUsage")}</p>
        </div>
      </div>
    </section>
  );
}
