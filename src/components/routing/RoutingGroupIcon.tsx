import { cloneElement, isValidElement } from "react";
import { Box } from "lucide-react";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import { hasIcon, isUrlIcon } from "@/icons/extracted";
import { useAccountText } from "@/components/AccountLabel";

export function RoutingGroupIcon({
  client,
  icons,
}: {
  client?: string | null;
  icons?: Array<{ icon?: string | null; name: string }>;
}) {
  const accountText = useAccountText();
  const unique = [
    ...new Map(
      (icons ?? []).map((entry) => [entry.icon || entry.name, entry]),
    ).values(),
  ];
  if (unique.length)
    return (
      <span className="mp-icon-stack" aria-hidden="true">
        {unique.slice(0, 3).map((entry) => (
          <span className="mp-icon-disc" key={entry.icon || entry.name}>
            <ProviderIcon
              icon={
                entry.icon && hasIcon(entry.icon) && !isUrlIcon(entry.icon)
                  ? entry.icon
                  : undefined
              }
              name={accountText(entry.name)}
              size={unique.length === 1 ? 20 : 13}
            />
          </span>
        ))}
        {unique.length > 3 && (
          <span className="mp-icon-disc more">+{unique.length - 3}</span>
        )}
      </span>
    );
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
