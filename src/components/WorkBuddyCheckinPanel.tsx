import { useEffect, useId, useState } from "react";
import { ChevronDown } from "lucide-react";
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

const queryKey = ["workbuddyCheckin", "accounts"];

export function WorkBuddyCheckinPanel() {
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
    !["claimed", "credentials_protected", "credentials_unavailable"].includes(
      data.outcome,
    );
  const hasReward =
    data && ["claimed", "already_checked"].includes(data.outcome);

  return (
    <section
      aria-label={t("workbuddy.checkin.title")}
      className="rounded-2xl border border-slate-200 bg-white p-5 dark:border-border dark:bg-card"
    >
      <h2>
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full items-center justify-between gap-3 text-left font-semibold"
        >
          {t("workbuddy.checkin.title")}
          <ChevronDown
            aria-hidden="true"
            className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`}
          />
        </button>
      </h2>
      <div id={detailsId} hidden={!expanded} className="mt-4 space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="flex-1 text-sm text-muted-foreground">
            {t("workbuddy.checkin.description")}
          </p>
          <div className="flex gap-2">
            <Button
              variant="ghost"
              disabled={busy || status.isFetching}
              onClick={() => void status.refetch()}
            >
              {t("workbuddy.checkin.reload")}
            </Button>
            <Button
              variant="outline"
              disabled={!canClaim || busy || status.isError}
              onClick={() => {
                auto.reset();
                if (selected) claim.mutate(selected.id);
              }}
            >
              {t(
                claim.isPending
                  ? "workbuddy.checkin.checking"
                  : "workbuddy.checkin.checkNow",
              )}
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div className="min-w-48 flex-1 space-y-1">
            <label htmlFor={accountSelectId} className="text-sm font-medium">
              {t("workbuddy.checkin.account")}
            </label>
            <select
              id={accountSelectId}
              className="block w-full rounded-md border bg-background p-2 text-sm"
              value={selected?.id ?? ""}
              disabled={busy || !status.data?.accounts.length}
              onChange={(event) => {
                setSelectedId(event.target.value);
                claim.reset();
                auto.reset();
              }}
            >
              {!selected && (
                <option value="">{t("workbuddy.checkin.selectAccount")}</option>
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
          <Button
            variant="outline"
            disabled={busy || !!loginId}
            onClick={() => add.mutate()}
          >
            {t("workbuddy.checkin.addAccount")}
          </Button>
          {selected?.source === "browser" && (
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => remove.mutate(selected.id)}
            >
              {t("workbuddy.checkin.removeAccount")}
            </Button>
          )}
        </div>
        {loginId && (
          <div className="flex items-center gap-3 text-sm">
            <p role="status">
              {t(
                login.isError
                  ? "workbuddy.checkin.authError"
                  : "workbuddy.checkin.loginWaiting",
              )}
            </p>
            <Button
              variant="outline"
              disabled={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              {t("common.cancel")}
            </Button>
          </div>
        )}
        {status.isError ? (
          <p role="alert" className="text-sm text-destructive">
            {t("workbuddy.checkin.loadFailed")}
          </p>
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
          <p role="alert" className="text-sm text-destructive">
            {t("workbuddy.checkin.operationFailed")}
          </p>
        )}
      </div>
    </section>
  );
}
