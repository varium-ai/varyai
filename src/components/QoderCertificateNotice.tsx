import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { proxyKeys } from "@/lib/query/proxy";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "sonner";
import { qoderCertificateErrorDetail } from "@/lib/qoderCertificateErrors";

export function QoderCertificateNotice() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [renewing, setRenewing] = useState(false);
  const [renewalFailed, setRenewalFailed] = useState(false);
  const { data } = useQuery({
    queryKey: ["qoderCertificateStatus"],
    queryFn: () =>
      invoke<{ expires_at: number | null; expired: boolean }>(
        "get_qoder_certificate_status",
      ),
    refetchInterval: 2000,
    refetchIntervalInBackground: true,
  });
  const expired = data?.expired === true;
  useEffect(() => {
    if (expired)
      void queryClient.invalidateQueries({
        queryKey: proxyKeys.takeoverStatus,
      });
  }, [expired, queryClient]);
  const close = () => {
    setDismissed(data?.expires_at ?? null);
    setRenewalFailed(false);
  };
  const renew = async () => {
    if (renewing) return;
    setRenewing(true);
    setRenewalFailed(false);
    try {
      await proxyApi.setProxyTakeoverForApp("qoder", true);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["qoderCertificateStatus"] }),
        queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus }),
        queryClient.invalidateQueries({ queryKey: proxyKeys.status }),
      ]);
      close();
      toast.success(t("qoder.certificate.renewed"), { closeButton: true });
    } catch (error) {
      setRenewalFailed(true);
      toast.error(t("qoder.certificate.renewFailed"), {
        description: qoderCertificateErrorDetail(error, t),
      });
    } finally {
      setRenewing(false);
    }
  };
  return (
    <AppDialog
      open={
        renewing || renewalFailed || (expired && data?.expires_at !== dismissed)
      }
      onOpenChange={(open) => {
        if (!open && !renewing) close();
      }}
      pending={renewing}
      zIndex="alert"
      title={t("qoder.certificate.expiredTitle")}
      description={t("qoder.certificate.expiredDescription")}
      footer={
        <>
          <Button variant="outline" disabled={renewing} onClick={close}>
            {t("common.confirm")}
          </Button>
          <Button disabled={renewing} onClick={() => void renew()}>
            {t(
              renewing
                ? "qoder.certificate.renewing"
                : "qoder.certificate.renew",
            )}
          </Button>
        </>
      }
    />
  );
}
