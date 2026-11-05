import { useEffect, useId, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  agentRewardsApi,
  type RewardApp,
  type AgentRewardStatus,
} from "@/lib/api/agent-rewards";
import { settingsApi } from "@/lib/api/settings";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import "./daily-checkin.css";

export function AgentRewardsPanel({
  app,
  embedded = false,
}: {
  app: RewardApp;
  embedded?: boolean;
}) {
  const { t } = useTranslation();
  const detailsId = useId();
  const accountId = useId();
  const autoId = useId();
  const [expanded, setExpanded] = useState(false);
  const [loginId, setLoginId] = useState<string | null>(null);
  const client = useQueryClient();
  const queryKey = ["agentRewards", app];
  const query = useQuery({
    queryKey,
    queryFn: () => agentRewardsApi.status(app),
    refetchInterval: 60_000,
  });
  const update = (value: AgentRewardStatus) =>
    client.setQueryData(queryKey, value);
  const claim = useMutation({
    mutationFn: (id: string) => agentRewardsApi.claim(app, id),
    onSuccess: update,
  });
  const auto = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      agentRewardsApi.setAuto(app, id, enabled),
    onSuccess: update,
  });
  const native = useMutation({
    mutationFn: agentRewardsApi.nativeAccount,
    onSuccess: update,
  });
  const authorize = useMutation({
    mutationFn: async () => {
      const login = await agentRewardsApi.startLogin(app);
      try {
        await settingsApi.openExternal(login.url);
      } catch {
        await agentRewardsApi.cancelLogin(app, login.loginId);
        throw new Error("agentRewards.operationFailed");
      }
      return login;
    },
    onSuccess: (login) => setLoginId(login.loginId),
  });
  const login = useQuery({
    queryKey: ["agentRewardsLogin", app, loginId],
    queryFn: () => agentRewardsApi.pollLogin(app, loginId!),
    enabled: !!loginId,
    retry: false,
    refetchInterval: (state) =>
      state.state.error || state.state.data?.complete ? false : 2000,
  });
  useEffect(() => {
    if (login.data?.complete) {
      setLoginId(null);
      void client.invalidateQueries({ queryKey: ["agentRewards", app] });
    }
  }, [login.data, client, app]);
  const cancel = useMutation({
    mutationFn: () => agentRewardsApi.cancelLogin(app, loginId!),
    onSuccess: () => setLoginId(null),
  });
  const forget = useMutation({
    mutationFn: (id: string) => agentRewardsApi.forget(app, id),
    onSuccess: () => client.invalidateQueries({ queryKey }),
  });
  const data = query.data;
  const browserAccountId =
    data?.browserAccountId ??
    (data?.source !== "native" ? data?.accountId : null);
  const busy =
    claim.isPending ||
    auto.isPending ||
    authorize.isPending ||
    cancel.isPending ||
    forget.isPending ||
    native.isPending ||
    !!loginId;
  const complete =
    data && ["claimed", "already_checked"].includes(data.outcome);
  const hasError =
    claim.isError ||
    auto.isError ||
    authorize.isError ||
    cancel.isError ||
    forget.isError ||
    native.isError;
  return (
    <section aria-label={t("agentRewards.title")} className="mp-checkin">
      <h3 hidden={embedded} className="mp-checkin-heading">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="mp-checkin-toggle"
        >
          {t("agentRewards.title")}
          <ChevronDown aria-hidden="true" />
        </button>
      </h3>
      <div
        id={detailsId}
        hidden={!embedded && !expanded}
        className="mp-checkin-body"
      >
        <div className="mp-checkin-intro">
          <p>{t(`agentRewards.descriptions.${app}`)}</p>
          <div className="mp-checkin-actions">
            <Button
              variant="ghost"
              size="sm"
              className="mp-checkin-button mp-checkin-quiet"
              disabled={busy || query.isFetching}
              onClick={() => void query.refetch()}
            >
              {t("agentRewards.refresh")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="mp-checkin-button mp-checkin-primary"
              disabled={
                busy ||
                query.isFetching ||
                query.isError ||
                !data?.accountId ||
                data.outcome !== "not_checked"
              }
              onClick={() => {
                auto.reset();
                if (data?.accountId) claim.mutate(data.accountId);
              }}
            >
              {t(
                claim.isPending
                  ? "agentRewards.claiming"
                  : "agentRewards.claim",
              )}
            </Button>
          </div>
        </div>
        <div className="mp-checkin-accounts">
          <div className="mp-checkin-field">
            <label htmlFor={accountId}>{t("agentRewards.account")}</label>
            <select id={accountId} value={data?.accountId ?? ""} disabled>
              <option value={data?.accountId ?? ""}>
                {data?.label ?? t("agentRewards.outcomes.sign_in_required")}
              </option>
            </select>
          </div>
          {app === "qoder" && (
            <>
              {data?.nativeAvailable &&
                (data.source !== "native" || !data.accountId) && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="mp-checkin-button"
                    disabled={busy || query.isFetching}
                    onClick={() => native.mutate()}
                  >
                    {t("agentRewards.readNative")}
                  </Button>
                )}
              <Button
                variant="outline"
                size="sm"
                className="mp-checkin-button"
                disabled={busy}
                onClick={() => authorize.mutate()}
              >
                {t("agentRewards.authorize")}
              </Button>
              {browserAccountId && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="mp-checkin-button mp-checkin-quiet"
                  disabled={busy}
                  onClick={() => forget.mutate(browserAccountId)}
                >
                  {t("agentRewards.forget")}
                </Button>
              )}
            </>
          )}
        </div>
        {app === "qoder" && data?.source && data.accountId && (
          <p className="mp-checkin-muted">
            {t(`agentRewards.sources.${data.source}`)}
          </p>
        )}
        {app === "qoder" &&
          data?.nativeAvailable &&
          (data.source !== "native" || !data.accountId) && (
            <p className="mp-checkin-muted">
              {t("agentRewards.nativeAccessHint")}
            </p>
          )}
        {loginId && (
          <div className="mp-checkin-actions">
            <p role="status">
              {t(
                login.isError
                  ? "agentRewards.loginFailed"
                  : "agentRewards.loginWaiting",
              )}
            </p>
            <Button
              variant="ghost"
              size="sm"
              className="mp-checkin-button"
              disabled={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              {t("common.cancel")}
            </Button>
          </div>
        )}
        {query.isError ? (
          <p role="alert" className="mp-checkin-error">
            {t("agentRewards.operationFailed")}
          </p>
        ) : data ? (
          <div role="status" className="mp-checkin-status">
            <p className="mp-checkin-outcome" data-complete={complete}>
              {t(`agentRewards.outcomes.${data.outcome}`)}
            </p>
            {data.credit != null && (
              <p>
                {t(
                  complete ? "agentRewards.reward" : "agentRewards.available",
                  { credit: data.credit },
                )}
              </p>
            )}
            {data.checkedAt != null && (
              <p className="mp-checkin-muted">
                {t("agentRewards.lastChecked", {
                  time: new Date(data.checkedAt).toLocaleString(),
                })}
              </p>
            )}
          </div>
        ) : (
          <p role="status" className="mp-checkin-muted">
            {t("common.loading")}
          </p>
        )}
        <div className="mp-checkin-auto">
          <div>
            <label htmlFor={autoId}>{t("agentRewards.autoLabel")}</label>
            <p className="mp-checkin-muted">{t("agentRewards.autoHint")}</p>
          </div>
          <Switch
            id={autoId}
            className="mp-switch"
            checked={data?.autoEnabled ?? false}
            disabled={
              !data?.accountId || busy || query.isFetching || query.isError
            }
            onCheckedChange={(enabled) => {
              claim.reset();
              if (data?.accountId) auto.mutate({ id: data.accountId, enabled });
            }}
          />
        </div>
        {hasError && (
          <p role="alert" className="mp-checkin-error">
            {t("agentRewards.operationFailed")}
          </p>
        )}
      </div>
    </section>
  );
}
