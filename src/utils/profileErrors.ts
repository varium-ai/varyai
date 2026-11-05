import { APP_IDS } from "@/config/appConfig";
import { extractErrorMessage } from "./errorUtils";

export function profileErrorMessage(
  value: unknown,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const message = extractErrorMessage(value);
  if (message.startsWith("profiles.bundle.agentError:")) {
    const match = message.match(
      /^profiles\.bundle\.agentError:([a-z-]+):(profiles\.bundle\.[a-zA-Z]+)$/,
    );
    const reasons = [
      "noProvider",
      "missingResource",
      "invalidConfig",
      "readFailed",
      "nativeModelMissing",
      "operationFailed",
    ].map((key) => `profiles.bundle.${key}`);
    if (
      match &&
      (APP_IDS as readonly string[]).includes(match[1]) &&
      reasons.includes(match[2])
    ) {
      return t("profiles.bundle.agentPreviewFailed", {
        app: t(`apps.${match[1]}`),
        reason: t(match[2]),
      });
    }
    return t("profiles.bundle.operationFailed");
  }
  const key = message.match(/profiles\.bundle\.[a-zA-Z]+/)?.[0];
  return t(key ?? "profiles.bundle.operationFailed");
}
