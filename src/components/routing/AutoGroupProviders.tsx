import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  intentRoutingApi,
  type RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

export function AutoGroupProviders({
  providers,
  disabled,
}: {
  providers: RoutingProviderOption[];
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const key = ["routing-auto-group-skip"];
  const skip = useQuery({
    queryKey: key,
    queryFn: intentRoutingApi.autoGroupSkip,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  async function change(id: string, excluded: boolean) {
    if (disabled || busy || !skip.data) return;
    const ids = excluded
      ? [...new Set([...skip.data, id])]
      : skip.data.filter((value) => value !== id);
    setBusy(true);
    setError(false);
    try {
      await intentRoutingApi.setAutoGroupSkip(ids);
      client.setQueryData(key, ids);
      await Promise.all(
        [
          "intent-routing-groups",
          "agent-model-catalog",
          "agent-connections",
          "intent-routing-client",
          "gateway-catalog",
        ].map((name) => client.invalidateQueries({ queryKey: [name] })),
      );
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="text mt-1 text-xs text-muted-foreground"
          disabled={disabled || busy || skip.isPending || skip.isError}
          title={t("intentRouting.skipHint")}
        >
          {skip.data?.length
            ? t("intentRouting.skippedProviders", { count: skip.data.length })
            : t("intentRouting.skipProviders")}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72">
        <p className="mb-2 text-xs text-muted-foreground">
          {t("intentRouting.skipHint")}
        </p>
        <div className="max-h-64 space-y-2 overflow-y-auto">
          {providers.map((provider) => (
            <label
              key={provider.id}
              className="flex items-center gap-2 text-sm"
            >
              <input
                type="checkbox"
                checked={skip.data?.includes(provider.id) ?? false}
                disabled={disabled || busy}
                onChange={(event) =>
                  void change(provider.id, event.target.checked)
                }
              />
              <span className="min-w-0 truncate">{provider.name}</span>
            </label>
          ))}
        </div>
        {error && (
          <p role="alert" className="mt-2 text-xs text-destructive">
            {t("intentRouting.failed")}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
