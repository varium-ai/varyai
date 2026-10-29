import { cloneElement, isValidElement } from "react";
import { Box } from "lucide-react";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";

export function RoutingGroupIcon({ client }: { client?: string | null }) {
  const appId = APP_IDS.find((id) => id === client);
  const app = appId ? APP_ICON_MAP[appId] : undefined;

  return (
    <span
      title={app?.label}
      className="hidden h-5 w-5 shrink-0 items-center justify-center text-muted-foreground sm:inline-flex"
    >
      {app && isValidElement<{ size?: number }>(app.icon) ? (
        cloneElement(app.icon, { size: 20 })
      ) : (
        <Box aria-hidden="true" className="h-5 w-5" />
      )}
    </span>
  );
}
