import { useMemo, useState } from "react";
import { ChevronDown, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  foldRoutingAccounts,
  type FoldedRoutingAccount,
} from "@/lib/routingAccounts";
import type { RoutingAccount, RoutingTrace } from "@/lib/api/routingTrace";

function preferences(): Record<string, boolean> {
  try {
    const parsed: unknown = JSON.parse(
      localStorage.getItem("vary.routingAccountFolds") ?? "{}",
    );
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(([, value]) => typeof value === "boolean"),
    );
  } catch {
    return {};
  }
}

export function RoutingAccounts({
  requests,
  current,
  live,
  refresh,
  refreshing,
  accountName,
}: {
  requests: RoutingTrace[];
  current: RoutingAccount[];
  live: boolean;
  refresh: () => void;
  refreshing: boolean;
  accountName: (candidate: {
    providerId: string;
    providerName: string;
  }) => string;
}) {
  const { t, i18n } = useTranslation();
  const [folds, setFolds] = useState(preferences);
  const accounts = useMemo(
    () => foldRoutingAccounts(requests, current, live),
    [requests, current, live],
  );
  const providers = new Map<string, FoldedRoutingAccount[]>();
  for (const account of accounts) {
    const group = providers.get(account.providerId) ?? [];
    group.push(account);
    providers.set(account.providerId, group);
  }
  const now = Date.now() / 1000;
  const clock = (value: number) =>
    new Date(value).toLocaleTimeString(i18n.language);
  function toggle(key: string, open: boolean) {
    const next = { ...folds, [key]: !open };
    setFolds(next);
    try {
      localStorage.setItem("vary.routingAccountFolds", JSON.stringify(next));
    } catch {
      /* Optional preferences. */
    }
  }
  function tally(rows: FoldedRoutingAccount[]) {
    const sum = (key: "tried" | "answered" | "failed") =>
      rows.reduce((n, row) => n + row[key], 0);
    const last = Math.max(0, ...rows.map((row) => row.lastAnswered ?? 0));
    const failures: Record<string, number> = {};
    for (const row of rows) {
      for (const [reason, count] of Object.entries(row.failures)) {
        failures[reason] = (failures[reason] ?? 0) + count;
      }
    }
    return (
      <div className="rt-account-tally">
        <span>{t("routingPage.accountTried", { n: sum("tried") })}</span>
        <span className="ok">
          {t("routingPage.accountAnswered", { n: sum("answered") })}
        </span>
        {Object.entries(failures).map(([reason, n]) => (
          <span className="bad" key={reason}>
            {t(
              reason === "slow"
                ? "routingPage.accountSlow"
                : reason === "context"
                  ? "routingPage.accountContext"
                  : reason === "failed"
                    ? "routingPage.accountFailed"
                    : "routingPage.accountFailureStatus",
              { n, status: reason },
            )}
          </span>
        ))}
        {last > 0 && (
          <span>
            {t("routingPage.accountLastAnswered", { time: clock(last) })}
          </span>
        )}
      </div>
    );
  }
  function models(rows: FoldedRoutingAccount[]) {
    const names = [...new Set(rows.flatMap((row) => row.models))];
    return (
      names.length > 0 && (
        <div className="rt-account-models">
          {names.map((model) => (
            <code key={model}>{model}</code>
          ))}
        </div>
      )
    );
  }
  function row(account: FoldedRoutingAccount, index: number, count: number) {
    const rests = account.observations.filter(
      (reading) => reading.restUntil > now,
    );
    const windows =
      live && account.kind === "account"
        ? (account.observations
            .find((reading) => reading.windows.length)
            ?.windows.filter((window) => window.resetsAt > now) ?? [])
        : [];
    return (
      <div key={account.id} className="rt-act" data-account-row>
        <div className="rt-account-name">
          <b>
            {account.kind === "provider"
              ? account.observations[0]?.model
              : t(
                  account.kind === "account"
                    ? "routingPage.accountLabel"
                    : "routingPage.keyLabel",
                  { n: count > 1 ? index + 1 : "" },
                ).trim()}
          </b>
        </div>
        {rests.length > 0 && (
          <div className="rt-account-rest">
            {rests.map((reading) => (
              <span key={reading.model}>
                {reading.model} ·{" "}
                {t("routingPage.restUntil", {
                  time: clock(reading.restUntil * 1000),
                })}
              </span>
            ))}
          </div>
        )}
        {account.observations.some((reading) => reading.sunkAt != null) && (
          <small>{t("routingPage.sunk")}</small>
        )}
        {windows.length ? (
          <div className="rt-account-windows">
            {windows.map((window) => (
              <span key={window.windowSeconds}>
                {t("routingPage.quotaUsed", {
                  hours: +(window.windowSeconds / 3600).toFixed(1),
                  percent: Math.round(window.usedPercent),
                })}
                {" · "}
                {t("routingPage.resetsAt", {
                  time: clock(window.resetsAt * 1000),
                })}
              </span>
            ))}
          </div>
        ) : (
          live &&
          account.kind === "account" && (
            <small>{t("routingPage.accountQuotaUnknown")}</small>
          )
        )}
        {tally([account])}
        {account.kind !== "provider" && models([account])}
        {windows.length > 0 && (
          <div
            className="bar"
            role="progressbar"
            aria-label={t("routingPage.accountQuotaUsed")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.max(
              ...windows.map((window) => window.usedPercent),
            )}
          >
            <i
              style={{
                width: `${Math.max(0, Math.min(100, Math.max(...windows.map((window) => window.usedPercent))))}%`,
              }}
            />
          </div>
        )}
      </div>
    );
  }
  return (
    <section className="rt-col" aria-label={t("routingPage.accounts")}>
      <div className="row-head">
        <h2 className="label">{t("routingPage.accounts")}</h2>
        {live && accounts.some((account) => account.kind === "account") && (
          <button
            type="button"
            className="text rt-read"
            aria-label={t("routingPage.refreshAccounts")}
            disabled={refreshing}
            onClick={refresh}
          >
            <RefreshCw size={12} className={refreshing ? "animate-spin" : ""} />
          </button>
        )}
        <span className="grow" />
        <span className="note">{t("routingPage.overRequests")}</span>
      </div>
      <div className="list rt-acts">
        {!accounts.length && (
          <p className="none">{t("routingPage.noAccounts")}</p>
        )}
        {[...providers].map(([providerId, rows]) => (
          <div key={providerId} className="rt-account-provider">
            <h3 className="rt-account-provider-name">
              {accountName({ providerId, providerName: rows[0].providerName })}
            </h3>
            {(["account", "key", "provider"] as const).map((kind) => {
              const entries = rows.filter((row) => row.kind === kind);
              const key = JSON.stringify([providerId, kind]);
              const fold = kind !== "provider" && entries.length >= 3;
              const open =
                folds[key] ?? (kind === "account" && entries.length < 6);
              return (
                <div key={kind}>
                  {fold && (
                    <button
                      type="button"
                      className="rt-act rt-account-fold"
                      aria-expanded={open}
                      title={t(
                        open
                          ? "routingPage.foldAccounts"
                          : "routingPage.expandAccounts",
                      )}
                      onClick={() => toggle(key, open)}
                    >
                      <b>
                        {t(
                          kind === "account"
                            ? "routingPage.accountCount"
                            : "routingPage.keyCount",
                          { n: entries.length },
                        )}
                        <ChevronDown size={12} />
                      </b>
                      <small>
                        {/* Model cooldowns are not account-wide rests. */}
                        {t("routingPage.accountsAvailable", {
                          n: entries.length,
                        })}
                      </small>
                      {tally(entries)}
                      {models(entries)}
                    </button>
                  )}
                  {(!fold || open) &&
                    entries.map((account, index) =>
                      row(account, index, entries.length),
                    )}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </section>
  );
}
