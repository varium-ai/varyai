import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export function SecurityEventsLoading() {
  const { t } = useTranslation();
  return (
    <div
      role="status"
      data-testid="security-events-loading"
      className="flex min-h-40 flex-col items-center justify-center gap-2 px-4 py-14 text-center text-xs text-muted-foreground"
    >
      <Loader2 className="h-5 w-5 animate-spin" aria-hidden="true" />
      {t("security.loadingLogs")}
    </div>
  );
}
