import { useTranslation } from "react-i18next";

export function GatewayRuntimeStatus({
  running,
  unavailable,
}: {
  running?: boolean;
  unavailable?: boolean;
}) {
  const { t } = useTranslation();
  const status = unavailable ? "unavailable" : running ? "running" : "stopped";

  return (
    <span className="gateway-runtime-status" data-status={status}>
      <span className="gateway-runtime-dot" aria-hidden="true" />
      {t(
        status === "unavailable"
          ? "workspaceNav.gatewayUnavailable"
          : status === "running"
            ? "workspaceNav.gatewayRunning"
            : "workspaceNav.gatewayStopped",
      )}
    </span>
  );
}
