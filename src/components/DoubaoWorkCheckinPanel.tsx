import { useId, useState } from "react";
import { CheckCircle2, ChevronDown } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  doubaoCheckinApi,
  type DoubaoCheckinStatus,
} from "@/lib/api/doubao-checkin";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  CheckinAccountTools,
  CheckinNotice,
  CheckinProgress,
} from "./CheckinProgress";
import "./daily-checkin.css";

const queryKey = ["doubaoCheckin"];

export function DoubaoWorkCheckinPanel({
  embedded = false,
}: {
  embedded?: boolean;
}) {
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
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const selected =
    query.data?.accounts.find(
      (account) =>
        account.id ===
        (selectedId ?? query.data.accounts.find((a) => a.current)?.id),
    ) ?? (!query.data?.connected ? query.data?.accounts[0] : undefined);
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
    <section aria-label={t("doubaowork.checkin.title")} className="mp-checkin">
      <h3 hidden={embedded} className="mp-checkin-heading">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="mp-checkin-toggle"
        >
          {t("doubaowork.checkin.title")}
          <ChevronDown aria-hidden="true" />
        </button>
      </h3>
      <div
        id={detailsId}
        hidden={!embedded && !expanded}
        className="mp-checkin-body"
      >
        <div className="mp-checkin-intro">
          <div className="mp-checkin-actions">
            <Button
              variant="outline"
              size="sm"
              className="mp-checkin-button mp-checkin-primary"
              data-complete={hasReward}
              disabled={!canClaim || busy || query.isError}
              onClick={() => {
                auto.reset();
                if (selected) claim.mutate(selected.id);
              }}
            >
              {hasReward && <CheckCircle2 size={15} aria-hidden="true" />}
              {hasReward
                ? t(
                    selected?.credit != null
                      ? "dailyCheckin.todayReward"
                      : "dailyCheckin.todayChecked",
                    { credit: selected?.credit },
                  )
                : t(
                    claim.isPending
                      ? "doubaowork.checkin.checking"
                      : selected && !selected.current
                        ? "doubaowork.checkin.switchAndCheckin"
                        : "doubaowork.checkin.checkNow",
                  )}
            </Button>
          </div>
        </div>
        <div className="mp-checkin-accounts">
          <div className="mp-checkin-field">
            <label htmlFor={accountId}>{t("doubaowork.checkin.account")}</label>
            <select
              id={accountId}
              value={selected?.id ?? ""}
              disabled={busy || !query.data?.accounts.length}
              onChange={(event) => {
                setSelectedId(event.target.value);
                claim.reset();
                auto.reset();
              }}
            >
              {!selected && (
                <option value="">
                  {t("doubaowork.checkin.selectAccount")}
                </option>
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
          <CheckinAccountTools>
            <p>{t("doubaowork.checkin.description")}</p>
            <Button
              variant="ghost"
              size="sm"
              className="mp-checkin-button mp-checkin-quiet"
              disabled={busy || query.isFetching}
              onClick={() => void query.refetch()}
            >
              {t("doubaowork.checkin.reload")}
            </Button>
          </CheckinAccountTools>
        </div>
        {selected && !selected.current && canClaim && (
          <p className="mp-checkin-muted">
            {t("doubaowork.checkin.switchHint")}
          </p>
        )}
        {query.isError ? (
          <p role="alert" className="mp-checkin-error">
            {t("doubaowork.checkin.loadFailed")}
          </p>
        ) : query.data ? (
          <div
            role="status"
            className="mp-checkin-status"
            data-complete={hasReward}
          >
            <p className="mp-checkin-outcome" data-complete={hasReward}>
              {t(
                `doubaowork.checkin.outcomes.${selected?.outcome ?? query.data.outcome}`,
              )}
            </p>
            {!query.data.connected && selected && (
              <p className="mp-checkin-muted">
                {t("agentRewards.savedResult")}{" "}
                {t(`doubaowork.checkin.outcomes.${query.data.outcome}`)}
              </p>
            )}
            {hasReward && selected.totalDays != null && (
              <p className="mp-checkin-muted">
                {t("doubaowork.checkin.totalDays", {
                  count: selected.totalDays,
                })}
              </p>
            )}
          </div>
        ) : (
          <p role="status" className="mp-checkin-muted">
            {t("common.loading")}
          </p>
        )}
        <CheckinProgress streak={selected?.streak} />
        <div className="mp-checkin-auto">
          <div>
            <label htmlFor={autoId}>{t("doubaowork.checkin.autoLabel")}</label>
            <p className="mp-checkin-muted">
              {t("doubaowork.checkin.autoHint")}
            </p>
          </div>
          <Switch
            id={autoId}
            className="mp-switch"
            checked={
              selected?.autoEnabled ?? query.data?.autoConfigured ?? false
            }
            disabled={
              !query.data?.connected || !selected || busy || query.isError
            }
            onCheckedChange={(enabled) => {
              claim.reset();
              if (selected) auto.mutate({ id: selected.id, enabled });
            }}
          />
        </div>
        {(claim.isError || auto.isError) && (
          <p role="alert" className="mp-checkin-error">
            {t("doubaowork.checkin.operationFailed")}
          </p>
        )}
        <CheckinNotice />
      </div>
    </section>
  );
}
