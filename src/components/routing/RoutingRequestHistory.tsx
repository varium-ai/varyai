import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { usageApi } from "@/lib/api/usage";
import { Button } from "@/components/ui/button";

export function RoutingRequestHistory() {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<string | null>(null);
  const requests = useQuery({
    queryKey: ["routing-request-history"],
    queryFn: () => usageApi.getRequestLogs({}, 0, 8),
    refetchInterval: 5000,
  });
  const request = requests.data?.data.find((row) => row.requestId === selected);
  return (
    <section className="mb-7 overflow-hidden rounded-xl border border-slate-200 bg-white dark:border-border dark:bg-card">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h2 className="text-sm font-medium">{t("modelPicker.history")}</h2>
        <Button
          variant="ghost"
          size="sm"
          disabled={requests.isFetching}
          onClick={() => void requests.refetch()}
        >
          {t("intentRouting.reload")}
        </Button>
      </div>
      {requests.isError ? (
        <p role="alert" className="p-4 text-sm text-muted-foreground">
          {t("modelPicker.historyFailed")}
        </p>
      ) : requests.isPending ? (
        <p role="status" className="p-4 text-sm">
          {t("common.loading")}
        </p>
      ) : !requests.data?.data.length ? (
        <p className="p-5 text-sm text-muted-foreground">
          {t("modelPicker.historyEmpty")}
        </p>
      ) : (
        <div className="divide-y">
          {requests.data.data.map((row) => (
            <button
              key={row.requestId}
              aria-expanded={selected === row.requestId}
              onClick={() =>
                setSelected(selected === row.requestId ? null : row.requestId)
              }
              className={`flex w-full items-center gap-3 px-4 py-3 text-left text-sm hover:bg-muted/40 ${selected === row.requestId ? "bg-blue-50/60 dark:bg-muted" : ""}`}
            >
              <span
                aria-hidden
                className={`h-1.5 w-1.5 shrink-0 rounded-full ${row.statusCode < 400 ? "bg-emerald-500" : "bg-amber-500"}`}
              />
              <span className="min-w-0 flex-1 truncate">
                {row.requestModel || row.model}
              </span>
              <span className="hidden text-xs text-muted-foreground sm:inline">
                {row.appType}
              </span>
              <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                {row.latencyMs} ms
              </span>
              <time
                className="shrink-0 text-xs text-muted-foreground"
                dateTime={new Date(row.createdAt * 1000).toISOString()}
              >
                {new Date(row.createdAt * 1000).toLocaleTimeString(undefined, {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </time>
            </button>
          ))}
        </div>
      )}
      {request && (
        <dl className="grid gap-3 border-t bg-muted/20 px-4 py-4 text-sm sm:grid-cols-2">
          {[
            [t("intentRouting.model"), request.model],
            [
              t("intentRouting.provider"),
              request.providerName || request.providerId,
            ],
            [t("modelPicker.latency"), `${request.latencyMs} ms`],
            [
              t("modelPicker.tokens"),
              `${request.inputTokens} / ${request.outputTokens}`,
            ],
          ].map(([label, value]) => (
            <div key={label} className="min-w-0">
              <dt className="text-xs text-muted-foreground">{label}</dt>
              <dd className="mt-1 break-words">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
