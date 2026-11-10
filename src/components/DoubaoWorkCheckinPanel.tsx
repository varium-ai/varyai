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
          <p>{t("doubaowork.checkin.description")}</p>
          <div className="mp-checkin-actions">
            <Button
              variant="ghost"
              size="sm"
              className="mp-checkin-button mp-checkin-quiet"
              disabled={busy || query.isFetching}
              onClick={() => void query.refetch()}
            >
              {t("doubaowork.checkin.reload")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="mp-checkin-button mp-checkin-primary"
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
          <p className="mp-checkin-muted">
            {t("doubaowork.checkin.switchHint")}
          </p>
        )}
        {query.isError ? (
          <p role="alert" className="mp-checkin-error">
            {t("doubaowork.checkin.loadFailed")}
          </p>
        ) : query.data ? (
          <div role="status" className="mp-checkin-status">
            <p className="mp-checkin-outcome" data-complete={hasReward}>
              {t(
                `doubaowork.checkin.outcomes.${selected?.outcome ?? query.data.outcome}`,
              )}
            </p>
            {hasReward && (
              <div className="mp-checkin-rewards">
                {selected.credit != null && (
                  <p>
                    {t("doubaowork.checkin.reward", {
                      credit: selected.credit,
                    })}
                  </p>
                )}
                {selected.streak != null && (
                  <p>
                    {t("doubaowork.checkin.streak", { count: selected.streak })}
                  </p>
                )}
                {selected.totalDays != null && (
                  <p>
                    {t("doubaowork.checkin.totalDays", {
                      count: selected.totalDays,
                    })}
                  </p>
                )}
              </div>
            )}
            <p className="mp-checkin-muted">
              {t("doubaowork.checkin.day", { day: query.data.day })}
            </p>
          </div>
        ) : (
          <p role="status" className="mp-checkin-muted">
            {t("common.loading")}
          </p>
        )}
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
            checked={selected?.autoEnabled ?? false}
            disabled={!selected || busy || query.isError}
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
      </div>
    </section>
  );
}
