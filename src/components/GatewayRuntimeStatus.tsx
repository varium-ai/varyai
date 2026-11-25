import { CircleAlert, CircleCheck, CircleStop } from "lucide-react";
import { useTranslation } from "react-i18next";

const statusIcons = {
  running: CircleCheck,
  stopped: CircleStop,
  unavailable: CircleAlert,
};

export function GatewayRuntimeStatus({
  running,
  unavailable,
}: {
  running?: boolean;
  unavailable?: boolean;
}) {
  const { t } = useTranslation();
  const status = unavailable ? "unavailable" : running ? "running" : "stopped";
  const Icon = statusIcons[status];

  return (
    <span className="gateway-runtime-status" data-status={status}>
      <Icon className="gateway-runtime-icon" aria-hidden="true" />
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
