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
  | "importProviders"
  | "signIn"
  | "account"
  | "copy"
  | "openWebsite";

export type ProviderErrorHandler = (
  operation: ProviderOperation,
  cause?: unknown,
) => void;

export const PROVIDER_ERROR_DURATION_MS = 6000;

export function providerErrorMessage(
  t: TFunction,
  operation: ProviderOperation,
  cause?: unknown,
) {
  // Only recognized codes select localized text; backend details never reach the toast.
  const code =
    typeof cause === "string" ? cause.replace(/^routingUpstreams\./, "") : "";
  const detailedReasons = [
    "invalidName",
    "invalidBaseUrl",
    "invalidApiKey",
    "credentialReentryRequired",
    "endpointSettingsReentryRequired",
    "invalidProtocol",
    "invalidModelConfig",
    "invalidKeyPool",
    "invalidEndpointSettings",
    "invalidAuthentication",
    "keyLimit",
    "modelLimit",
    "configTooLarge",
  ];
  const reason =
    cause === "providerImport.conflict"
      ? "providerImport.conflict"
      : cause === "providerImport.invalid"
        ? "providerImport.invalid"
        : operation === "importProviders" && code === "nameExists"
          ? "providerImport.nameExists"
          : [
                "conflict",
                "limit",
                "referenced",
                "sourceUnavailable",
                "nameExists",
              ].includes(code)
            ? `modelProviders.${code}`
            : detailedReasons.includes(code)
              ? `modelProviders.errors.${code}`
              : code === "invalid"
                ? `modelProviders.errors.${operation === "importAccount" ? "invalidAccountFile" : "invalid"}`
                : code === "invalidModels"
                  ? "modelProviders.invalidModels"
                  : code === "emptyCatalog"
                    ? "intentRouting.catalogReason.empty"
                    : undefined;
  return {
    title: t(
      operation === "copy"
        ? "onboarding.copyFailed"
        : operation === "importProviders"
          ? "providerImport.failed"
          : `modelProviders.errors.${operation}`,
    ),
    description: reason ? t(reason) : undefined,
  };
}

export function showProviderError(
  t: TFunction,
  operation: ProviderOperation,
  cause?: unknown,
) {
  const { title, description } = providerErrorMessage(t, operation, cause);
  toast.error(title, {
    id: `provider-error-${operation}`,
    description,
    duration: PROVIDER_ERROR_DURATION_MS,
    closeButton: true,
  });
}
