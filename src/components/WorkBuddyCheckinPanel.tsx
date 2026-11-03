import { useId } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  workbuddyCheckinApi,
  type WorkBuddyCheckinStatus,
} from "@/lib/api/workbuddy-checkin";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";

const queryKey = ["workbuddyCheckin"];

export function WorkBuddyCheckinPanel() {
  const { t } = useTranslation();
  const toggleId = useId();
  const client = useQueryClient();
  const status = useQuery({
    queryKey,
    queryFn: workbuddyCheckinApi.status,
    refetchInterval: 60_000,
  });
  const update = (data: WorkBuddyCheckinStatus) =>
    client.setQueryData(queryKey, data);
  const claim = useMutation({
    mutationFn: workbuddyCheckinApi.checkIn,
    onSuccess: update,
  });
  const auto = useMutation({
    mutationFn: (enabled: boolean) =>
      workbuddyCheckinApi.setAutoEnabled(enabled),
    onSuccess: update,
  });
  const busy = claim.isPending || auto.isPending;
  const data = status.data;
  const canClaim =
    data && !["claimed", "credentials_unavailable"].includes(data.outcome);
  const hasReward =
    data && ["claimed", "already_checked"].includes(data.outcome);

  return (
    <section
      aria-label={t("workbuddy.checkin.title")}
      className="space-y-4 rounded-2xl border border-slate-200 bg-white p-5 dark:border-border dark:bg-card"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-semibold">{t("workbuddy.checkin.title")}</h2>
        <Button
          variant="outline"
          disabled={!canClaim || busy || status.isError}
          onClick={() => {
            auto.reset();
            claim.mutate();
          }}
        >
          {t(
            claim.isPending
              ? "workbuddy.checkin.checking"
              : "workbuddy.checkin.checkNow",
          )}
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        {t("workbuddy.checkin.description")}
      </p>
      {status.isError ? (
        <div className="flex items-center gap-3">
          <p role="alert" className="text-sm text-destructive">
            {t("workbuddy.checkin.loadFailed")}
          </p>
          <Button
            variant="outline"
            disabled={status.isFetching}
            onClick={() => void status.refetch()}
          >
            {t("workbuddy.checkin.reload")}
          </Button>
        </div>
      ) : data ? (
        <div role="status" className="space-y-1 text-sm">
          <p>{t(`workbuddy.checkin.outcomes.${data.outcome}`)}</p>
          {hasReward && data.credit !== null && (
            <p>{t("workbuddy.checkin.reward", { credit: data.credit })}</p>
          )}
          {hasReward && data.streak !== null && (
            <p>{t("workbuddy.checkin.streak", { count: data.streak })}</p>
          )}
          {hasReward && data.creditDelayed && (
            <p className="text-muted-foreground">
              {t("workbuddy.checkin.delayed")}
            </p>
          )}
          <p className="text-xs text-muted-foreground">
            {t("workbuddy.checkin.day", { day: data.day })}
          </p>
        </div>
      ) : (
        <p role="status" className="text-sm text-muted-foreground">
          {t("common.loading")}
        </p>
      )}
      <div className="flex items-center justify-between gap-4 border-t pt-3">
        <div className="space-y-1">
          <label htmlFor={toggleId} className="text-sm font-medium">
            {t("workbuddy.checkin.autoLabel")}
          </label>
          <p className="text-xs text-muted-foreground">
            {t("workbuddy.checkin.autoHint")}
          </p>
        </div>
        <Switch
          id={toggleId}
          checked={data?.autoEnabled ?? false}
          disabled={!data || busy || status.isError}
          onCheckedChange={(enabled) => {
            claim.reset();
            auto.mutate(enabled);
          }}
        />
      </div>
      {(claim.isError || auto.isError) && (
        <p role="alert" className="text-sm text-destructive">
          {t("workbuddy.checkin.operationFailed")}
        </p>
      )}
    </section>
  );
}
