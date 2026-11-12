import { toast } from "sonner";
import type { TFunction } from "i18next";

export type ProviderOperation =
  | "loadProviders"
  | "load"
  | "create"
  | "save"
  | "toggle"
  | "remove"
  | "fetchModels"
  | "testModels"
  | "importAccount"
  | "signIn"
  | "account"
  | "copy"
  | "openWebsite";

export type ProviderErrorHandler = (
  operation: ProviderOperation,
  cause?: unknown,
) => void;

export function showProviderError(
  t: TFunction,
  operation: ProviderOperation,
  cause?: unknown,
) {
  // Only recognized codes select localized text; backend details never reach the toast.
  const code =
    typeof cause === "string" ? cause.replace(/^routingUpstreams\./, "") : "";
  const reason = [
    "conflict",
    "limit",
    "referenced",
    "sourceUnavailable",
  ].includes(code)
    ? `modelProviders.${code}`
    : code === "invalid"
      ? `modelProviders.errors.${operation === "importAccount" ? "invalidAccountFile" : "invalid"}`
      : code === "invalidModels"
        ? "modelProviders.invalidModels"
        : undefined;
  toast.error(
    t(
      operation === "copy"
        ? "onboarding.copyFailed"
        : `modelProviders.errors.${operation}`,
    ),
    {
      id: `provider-error-${operation}`,
      description: reason ? t(reason) : undefined,
      duration: 6000,
      closeButton: true,
    },
  );
}
