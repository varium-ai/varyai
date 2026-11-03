import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  doubaoCheckinApi,
  type DoubaoCheckinStatus,
} from "@/lib/api/doubao-checkin";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";

const queryKey = ["doubaoCheckin"];

export function DoubaoWorkCheckinPanel() {
  const { t } = useTranslation();
  const accountId = useId();
  const autoId = useId();
  const detailsId = useId();
  const [expanded, setExpanded] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const client = useQueryClient();
  const query = useQuery({
    queryKey,
    queryFn: doubaoCheckinApi.accounts,
    refetchInterval: 60_000,
  });
  const selected = query.data?.accounts.find(
    (account) =>
      account.id ===
      (selectedId ?? query.data.accounts.find((a) => a.current)?.id),
  );
  const update = (value: DoubaoCheckinStatus) =>
    client.setQueryData(queryKey, value);
  const claim = useMutation({
    mutationFn: (id: string) => doubaoCheckinApi.checkIn(id),
    onSuccess: update,
  });
  const auto = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      doubaoCheckinApi.setAutoEnabled(id, enabled),
    onSuccess: update,
  });
  const busy = claim.isPending || auto.isPending;
  const canClaim =
    query.data?.connected &&
    selected &&
    !["claimed", "already_checked"].includes(selected.outcome);
  const hasReward =
    selected && ["claimed", "already_checked"].includes(selected.outcome);

  return (
    <section
      aria-label={t("doubaowork.checkin.title")}
      className="rounded-2xl border bg-card p-5"
    >
      <h2>
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full items-center justify-between gap-3 text-left font-semibold"
        >
          {t("doubaowork.checkin.title")}
          <ChevronDown
            aria-hidden="true"
            className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`}
          />
        </button>
      </h2>
      <div id={detailsId} hidden={!expanded} className="mt-4 space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="flex-1 text-sm text-muted-foreground">
            {t("doubaowork.checkin.description")}
          </p>
          <div className="flex gap-2">
            <Button
              variant="ghost"
              disabled={busy || query.isFetching}
              onClick={() => void query.refetch()}
            >
              {t("doubaowork.checkin.reload")}
            </Button>
            <Button
              variant="outline"
              disabled={!canClaim || busy || query.isError}
              onClick={() => {
                auto.reset();
                if (selected) claim.mutate(selected.id);
              }}
            >
              {t(
                claim.isPending
                  ? "doubaowork.checkin.checking"
                  : selected && !selected.current
                    ? "doubaowork.checkin.switchAndCheckin"
                    : "doubaowork.checkin.checkNow",
              )}
            </Button>
          </div>
        </div>
        <div className="space-y-1">
          <label htmlFor={accountId} className="text-sm font-medium">
            {t("doubaowork.checkin.account")}
          </label>
          <select
            id={accountId}
            className="block w-full rounded-md border bg-background p-2 text-sm"
            value={selected?.id ?? ""}
            disabled={busy || !query.data?.accounts.length}
            onChange={(event) => {
              setSelectedId(event.target.value);
              claim.reset();
              auto.reset();
            }}
          >
            {!selected && (
              <option value="">{t("doubaowork.checkin.selectAccount")}</option>
            )}
            {query.data?.accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.label}
                {account.current
                  ? ` (${t("doubaowork.checkin.currentAccount")})`
                  : ""}
              </option>
            ))}
          </select>
        </div>
        {selected && !selected.current && canClaim && (
          <p className="text-sm text-muted-foreground">
            {t("doubaowork.checkin.switchHint")}
          </p>
        )}
        {query.isError ? (
          <p role="alert" className="text-sm text-destructive">
            {t("doubaowork.checkin.loadFailed")}
          </p>
        ) : query.data ? (
          <div role="status" className="space-y-1 text-sm">
            <p>
              {t(
                `doubaowork.checkin.outcomes.${selected?.outcome ?? query.data.outcome}`,
              )}
            </p>
            {hasReward && selected.credit != null && (
              <p>
                {t("doubaowork.checkin.reward", { credit: selected.credit })}
              </p>
            )}
            {hasReward && selected.streak != null && (
              <p>
                {t("doubaowork.checkin.streak", { count: selected.streak })}
              </p>
            )}
            {hasReward && selected.totalDays != null && (
              <p>
                {t("doubaowork.checkin.totalDays", {
                  count: selected.totalDays,
                })}
              </p>
            )}
            <p className="text-xs text-muted-foreground">
              {t("doubaowork.checkin.day", { day: query.data.day })}
            </p>
          </div>
        ) : (
          <p role="status" className="text-sm text-muted-foreground">
            {t("common.loading")}
          </p>
        )}
        <div className="flex items-center justify-between gap-4 border-t pt-3">
          <div className="space-y-1">
            <label htmlFor={autoId} className="text-sm font-medium">
              {t("doubaowork.checkin.autoLabel")}
            </label>
            <p className="text-xs text-muted-foreground">
              {t("doubaowork.checkin.autoHint")}
            </p>
          </div>
          <Switch
            id={autoId}
            checked={selected?.autoEnabled ?? false}
            disabled={!selected || busy || query.isError}
            onCheckedChange={(enabled) => {
              claim.reset();
              if (selected) auto.mutate({ id: selected.id, enabled });
            }}
          />
        </div>
        {(claim.isError || auto.isError) && (
          <p role="alert" className="text-sm text-destructive">
            {t("doubaowork.checkin.operationFailed")}
          </p>
        )}
      </div>
    </section>
  );
}
