import { useId, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import {
  intentRoutingApi,
  type RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Checkbox } from "@/components/ui/checkbox";
import { AccountLabel, useAccountText } from "@/components/AccountLabel";

export function AutoGroupProviders({
  providers,
  disabled,
}: {
  providers: RoutingProviderOption[];
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const accountText = useAccountText();
  const descriptionId = useId();
  const client = useQueryClient();
  const key = ["routing-auto-group-skip"];
  const skip = useQuery({
    queryKey: key,
    queryFn: intentRoutingApi.autoGroupSkip,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const label = skip.data?.length
    ? t("intentRouting.skippedProviders", { count: skip.data.length })
    : t("intentRouting.skipProviders");
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
          className="text mt-1 max-w-full justify-self-start text-left"
          disabled={disabled || busy || skip.isPending || skip.isError}
          title={t("intentRouting.skipHint")}
        >
          <span className="min-w-0 truncate">{label}</span>
          <ChevronDown size={12} className="shrink-0" aria-hidden="true" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        sideOffset={6}
        collisionPadding={16}
        aria-label={t("intentRouting.skipProviders")}
        aria-describedby={descriptionId}
        className="flex max-h-[var(--radix-popover-content-available-height)] w-80 max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-lg border-[var(--mp-line)] bg-[var(--mp-card)] p-3 text-xs leading-relaxed text-[var(--mp-fg)] [font-family:var(--mp-font)]"
      >
        <p id={descriptionId} className="mb-2 shrink-0 text-[var(--mp-muted)]">
          {t("intentRouting.skipHint")}
        </p>
        <div className="min-h-0 max-h-64 space-y-1 overflow-y-auto">
          {providers.map((provider) => (
            <label
              key={provider.id}
              data-checked={skip.data?.includes(provider.id) ?? false}
              data-disabled={disabled || busy}
              className="flex min-h-8 cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-xs leading-5 hover:bg-[var(--mp-card-2)] focus-within:bg-[var(--mp-card-2)] data-[checked=true]:bg-[var(--mp-pill)] data-[disabled=true]:cursor-default data-[disabled=true]:opacity-50"
            >
              <Checkbox
                className="shrink-0 accent-[var(--mp-accent)]"
                checked={skip.data?.includes(provider.id) ?? false}
                disabled={disabled || busy}
                onCheckedChange={(checked) => void change(provider.id, checked)}
              />
              <span
                className="min-w-0 truncate"
                title={accountText(provider.name)}
              >
                <AccountLabel>{provider.name}</AccountLabel>
              </span>
            </label>
          ))}
        </div>
        {error && (
          <p role="alert" className="mt-2 shrink-0 text-[var(--mp-red)]">
            {t("intentRouting.failed")}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
