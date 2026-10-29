import type { TFunction } from "i18next";
import { extractErrorMessage } from "@/utils/errorUtils";

export function qoderCertificateErrorDetail(error: unknown, t: TFunction) {
  const message = extractErrorMessage(error);
  if (
    message.includes(
      "Qoder CN codec is unavailable or this client version is unsupported; the request was not forwarded",
    )
  ) {
    return t("qoder.certificate.failureDetail", {
      stage: t("qoder.certificate.failureStages.codec"),
      status: "unsupported",
    });
  }
  const match = message.match(
    /^Qoder certificate (data|generate|inspect|verify_trust|install_trust|remove_trust|delete_certificate) failed: (exit=-?\d{1,10}|terminated|launch|timeout|failed)$/,
  );
  if (!match) return undefined;
  return t("qoder.certificate.failureDetail", {
    stage: t(`qoder.certificate.failureStages.${match[1]}`),
    status: match[2],
  });
}
