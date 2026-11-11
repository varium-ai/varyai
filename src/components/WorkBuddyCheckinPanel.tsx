import { useEffect, useId, useState } from "react";
import { CheckCircle2, ChevronDown } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  workbuddyCheckinApi,
  type WorkBuddyCheckinAccounts,
  type WorkBuddyCheckinStatus,
} from "@/lib/api/workbuddy-checkin";
import { settingsApi } from "@/lib/api/settings";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  CheckinAccountTools,
  CheckinNotice,
  CheckinProgress,
} from "./CheckinProgress";
import "./daily-checkin.css";

const queryKey = ["workbuddyCheckin", "accounts"];

export function WorkBuddyCheckinPanel({
  embedded = false,
}: {
  embedded?: boolean;
}) {
  const { t } = useTranslation();
  const toggleId = useId();
  const accountSelectId = useId();
  const detailsId = useId();
  const [expanded, setExpanded] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loginId, setLoginId] = useState<string | null>(null);
  const client = useQueryClient();
  const status = useQuery({
    queryKey,
    queryFn: workbuddyCheckinApi.accounts,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const selected = status.data?.accounts.find(
    (account) => account.id === (selectedId ?? status.data.accounts[0]?.id),
  );
  const update = (data: WorkBuddyCheckinStatus, id: string) =>
    client.setQueryData<WorkBuddyCheckinAccounts>(
      queryKey,
      (previous) =>
        previous && {
          ...previous,
          nativeStatus: {
            ...previous.nativeStatus,
            autoConfigured: data.autoConfigured,
          },
          accounts: previous.accounts.map((account) =>
            account.id === id ? { ...account, status: data } : account,
          ),
        },
    );
  const claim = useMutation({
    mutationFn: (id: string) => workbuddyCheckinApi.checkIn(id),
    onSuccess: (data, id) => update(data, id),
  });
  const auto = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      workbuddyCheckinApi.setAutoEnabled(enabled, id),
    onSuccess: (data, { id }) => update(data, id),
  });
  const add = useMutation({
    mutationFn: async () => {
      const login = await workbuddyCheckinApi.startLogin();
      try {
        await settingsApi.openExternal(login.url);
      } catch {
        await workbuddyCheckinApi.cancelLogin(login.loginId);
        throw new Error("login failed");
      }
      return login;
    },
    onSuccess: (login) => setLoginId(login.loginId),
  });
  const login = useQuery({
    queryKey: ["workbuddyCheckinLogin", loginId],
    queryFn: () => workbuddyCheckinApi.pollLogin(loginId!),
    enabled: !!loginId,
    retry: false,
    refetchInterval: (query) =>
      query.state.error || query.state.data?.complete ? false : 2000,
  });
  useEffect(() => {
    if (login.data?.complete && login.data.accountId) {
      setSelectedId(login.data.accountId);
      setLoginId(null);
      void client.invalidateQueries({ queryKey });
    }
  }, [login.data, client]);
  const cancel = useMutation({
    mutationFn: () => workbuddyCheckinApi.cancelLogin(loginId!),
    onSuccess: () => setLoginId(null),
  });
  const remove = useMutation({
    mutationFn: workbuddyCheckinApi.removeAccount,
    onSuccess: () => {
      setSelectedId(null);
      void client.invalidateQueries({ queryKey });
    },
  });
  const busy =
    claim.isPending || auto.isPending || add.isPending || remove.isPending;
  const data = selected?.status ?? status.data?.nativeStatus;
  const canClaim =
    selected &&
    data &&
    ![
      "claimed",
      "already_checked",
      "credentials_protected",
      "credentials_unavailable",
    ].includes(data.outcome);
  const outcome = data?.savedOutcome ?? data?.outcome;
  const hasReward = outcome && ["claimed", "already_checked"].includes(outcome);

  return (
    <section aria-label={t("workbuddy.checkin.title")} className="mp-checkin">
      <h3 hidden={embedded} className="mp-checkin-heading">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="mp-checkin-toggle"
        >
          {t("workbuddy.checkin.title")}
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
              disabled={!canClaim || busy || status.isError}
              onClick={() => {
                auto.reset();
                if (selected) claim.mutate(selected.id);
              }}
            >
              {hasReward && <CheckCircle2 size={15} aria-hidden="true" />}
              {hasReward
                ? t(
                    data?.credit != null
                      ? "dailyCheckin.todayReward"
                      : "dailyCheckin.todayChecked",
                    { credit: data?.credit },
                  )
                : t(
                    claim.isPending
                      ? "workbuddy.checkin.checking"
                      : "workbuddy.checkin.checkNow",
                  )}
            </Button>
          </div>
        </div>
        <div className="mp-checkin-accounts">
          <div className="mp-checkin-field">
            <label htmlFor={accountSelectId}>
              {t("workbuddy.checkin.account")}
            </label>
            <select
              id={accountSelectId}
              value={selected?.id ?? ""}
              disabled={busy || !status.data?.accounts.length}
              onChange={(event) => {
                setSelectedId(event.target.value);
                claim.reset();
                auto.reset();
              }}
            >
              {!selected && (
                <option value="">
                  {t(
                    data?.savedOutcome
                      ? "agentRewards.savedAccount"
                      : "workbuddy.checkin.selectAccount",
                  )}
                </option>
              )}
              {status.data?.accounts.map((account) => (
                <option key={account.id} value={account.id}>
                  {account.label}
                  {account.source === "native"
                    ? ` (${t("workbuddy.checkin.nativeAccount")})`
                    : ""}
                </option>
              ))}
            </select>
          </div>
          <CheckinAccountTools>
            <p>{t("workbuddy.checkin.description")}</p>
            <Button
              variant="ghost"
              size="sm"
              className="mp-checkin-button mp-checkin-quiet"
              disabled={busy || status.isFetching}
              onClick={() => void status.refetch()}
            >
              {t("workbuddy.checkin.reload")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="mp-checkin-button"
              disabled={busy || !!loginId}
              onClick={() => add.mutate()}
            >
              {t("workbuddy.checkin.addAccount")}
            </Button>
            {selected?.source === "browser" && (
              <Button
                variant="ghost"
                size="sm"
                className="mp-checkin-button mp-checkin-quiet"
                disabled={busy}
                onClick={() => remove.mutate(selected.id)}
              >
                {t("workbuddy.checkin.removeAccount")}
              </Button>
            )}
          </CheckinAccountTools>
        </div>
        {loginId && (
          <div className="mp-checkin-actions">
            <p role="status">
              {t(
                login.isError
                  ? "workbuddy.checkin.authError"
                  : "workbuddy.checkin.loginWaiting",
              )}
            </p>
            <Button
              variant="outline"
              size="sm"
              className="mp-checkin-button"
              disabled={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              {t("common.cancel")}
            </Button>
          </div>
        )}
        {status.isError ? (
          <p role="alert" className="mp-checkin-error">
            {t("workbuddy.checkin.loadFailed")}
          </p>
        ) : data ? (
          <div
            role="status"
            className="mp-checkin-status"
            data-complete={hasReward}
          >
            <p className="mp-checkin-outcome" data-complete={hasReward}>
              {t(`workbuddy.checkin.outcomes.${outcome}`)}
            </p>
            {data.savedOutcome && (
              <p className="mp-checkin-muted">
                {t("agentRewards.savedResult")}{" "}
                {t(`workbuddy.checkin.outcomes.${data.outcome}`)}
              </p>
            )}

            {hasReward && data.creditDelayed && (
              <p className="mp-checkin-muted">
                {t("workbuddy.checkin.delayed")}
              </p>
            )}
          </div>
        ) : (
          <p role="status" className="mp-checkin-muted">
            {t("common.loading")}
          </p>
        )}
        <CheckinProgress streak={data?.streak} />
        <div className="mp-checkin-auto">
          <div>
            <label htmlFor={toggleId}>{t("workbuddy.checkin.autoLabel")}</label>
            <p className="mp-checkin-muted">
              {t("workbuddy.checkin.autoHint")}
            </p>
          </div>
          <Switch
            id={toggleId}
            className="mp-switch"
            checked={
              (selected ? data?.autoEnabled : data?.autoConfigured) ?? false
            }
            disabled={!selected || busy || status.isError}
            onCheckedChange={(enabled) => {
              claim.reset();
              if (selected) auto.mutate({ enabled, id: selected.id });
            }}
          />
        </div>
        {(claim.isError ||
          auto.isError ||
          add.isError ||
          cancel.isError ||
          remove.isError) && (
          <p role="alert" className="mp-checkin-error">
            {t("workbuddy.checkin.operationFailed")}
          </p>
        )}
        <CheckinNotice />
      </div>
    </section>
  );
}
