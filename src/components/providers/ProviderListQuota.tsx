import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
  type CodexProviderEditor,
} from "@/lib/api/routingUpstreams";
import { useCodexProviderQuotas } from "@/lib/query/codexProviderQuota";
import { ApiProviderQuota } from "./ApiProviderQuota";
import { SubscriptionAccountQuota } from "./SubscriptionAccountQuota";
import { SubscriptionQuotaSummary } from "./SubscriptionQuotaSummary";
import { ProviderQuotaCard, type QuotaCardOptions } from "./ProviderQuotaCard";
import { CodexQuotaControls } from "./CodexQuotaControls";

function CodexListQuota({
  record,
  card,
}: {
  record: RoutingUpstream;
  card?: QuotaCardOptions;
}) {
  const { t } = useTranslation();
  const selected = record.codexSettings?.accounts ??
    record.codex?.settings.accounts ?? ["native"];
  const observation = useQuery({
    queryKey: [
      "provider-list-codex-accounts",
      record.id,
      record.revision,
      record.reauthRequired,
    ],
    queryFn: () => routingUpstreamsApi.editor(record.id, record.revision),
    enabled:
      !record.codex &&
      record.reauthRequired !== undefined &&
      (!record.reauthRequired || selected.length > 1),
    staleTime: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const editor = record.codex ?? observation.data?.codex;
  const accounts: CodexProviderEditor["accounts"] = [];
  for (const id of editor?.settings.accounts ?? selected) {
    const account = editor?.accounts.find(
      (row) => row.id === id || row.aliases?.includes(id),
    );
    if (!accounts.some((row) => row.id === (account?.id ?? id))) {
      accounts.push(
        account ?? { id, login: id, native: id === "native", ready: false },
      );
    }
  }
  const queries = useCodexProviderQuotas(editor ? accounts : [], !!card);
  if (!editor) {
    if (card)
      return (
        <ProviderQuotaCard
          name={record.name}
          items={[]}
          card={card}
          fetching={observation.isFetching}
          loading={observation.isFetching}
          status={`codexProvider.${record.reauthRequired === true ? "quotaLoginRequired" : observation.isFetching ? "quotaLoading" : observation.isError ? "quotaFailed" : "quotaUnknown"}`}
          refresh={() => void observation.refetch({ cancelRefetch: false })}
        />
      );
    return (
      <SubscriptionQuotaSummary
        label={`${t("subscriptionProvider.allowance")} ${record.name}`}
        items={[]}
        fetching={observation.isFetching}
        loading={observation.isFetching}
        status={`codexProvider.${record.reauthRequired === true ? "quotaLoginRequired" : observation.isFetching ? "quotaLoading" : observation.isError ? "quotaFailed" : "quotaUnknown"}`}
      />
    );
  }
  const Container = card ? "div" : "span";
  return (
    <Container className="pv-list-quotas">
      {accounts.map((account, index) => {
        const query = queries[index];
        const quota = query.data?.quota;
        const valid =
          account.ready && quota?.credentialStatus === "valid" && quota.success;
        const tiers = valid
          ? quota.tiers.filter((tier) => Number.isFinite(tier.utilization))
          : [];
        const held =
          valid &&
          (quota.codex?.allowed === false ||
            quota.codex?.limitReached === true ||
            quota.codex?.spendControl?.reached === true);
        const balance = valid ? quota.codex?.credits?.balance : null;
        const status =
          !account.ready || quota?.credentialStatus === "expired"
            ? "quotaLoginRequired"
            : quota?.credentialStatus === "not_found"
              ? "quotaCredentialsMissing"
              : quota?.credentialStatus === "parse_error"
                ? "quotaCredentialsInvalid"
                : !query.data
                  ? "quotaLoading"
                  : query.data.failed
                    ? tiers.length
                      ? "quotaStale"
                      : "quotaFailed"
                    : !tiers.length && !held && balance == null
                      ? "quotaEmpty"
                      : null;
        if (card)
          return (
            <ProviderQuotaCard
              key={account.id}
              name={account.login}
              title={[
                record.name,
                account.login,
                account.plan ??
                  record.subscriptionPlans?.filter(Boolean).join(" · "),
              ]
                .filter(Boolean)
                .join(" · ")}
              card={card}
              items={tiers.map((tier) => ({
                tier,
                label: t(
                  `subscription.${tier.name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())}`,
                  { defaultValue: tier.name },
                ),
              }))}
              status={status ? `codexProvider.${status}` : null}
              fetching={query.isFetching}
              loading={!query.data && query.isFetching}
              disabled={!account.ready}
              sample={
                query.data?.observedAt && quota?.accountIdentity
                  ? {
                      scope: `codex:${quota.accountIdentity}`,
                      at: query.data.observedAt,
                    }
                  : undefined
              }
              refresh={() => void query.refetch({ cancelRefetch: false })}
            >
              {held && <span>{t("codexProvider.quotaHeld")}</span>}
              {balance != null && (
                <span>{t("codexProvider.creditBalance", { balance })}</span>
              )}
              {account.ready && (
                <CodexQuotaControls account={account.id} name={account.login} />
              )}
            </ProviderQuotaCard>
          );
        return (
          <span className="pv-list-quota-account" key={account.id}>
            {accounts.length > 1 && (
              <span className="pv-quota-account-label">{account.login}</span>
            )}
            <SubscriptionQuotaSummary
              label={`${t("subscriptionProvider.allowance")} ${record.name}`}
              fetching={query.isFetching}
              loading={!query.data && query.isFetching}
              items={tiers.map((tier) => ({
                tier,
                label: t(
                  `subscription.${tier.name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())}`,
                  { defaultValue: tier.name },
                ),
              }))}
              status={status ? `codexProvider.${status}` : null}
            >
              {held && <span>{t("codexProvider.quotaHeld")}</span>}
              {balance != null && (
                <span>{t("codexProvider.creditBalance", { balance })}</span>
              )}
            </SubscriptionQuotaSummary>
          </span>
        );
      })}
    </Container>
  );
}

export function ProviderListQuota({
  record,
  card,
}: {
  record: RoutingUpstream;
  card?: QuotaCardOptions;
}) {
  if (record.source.kind === "api")
    return <ApiProviderQuota record={record} card={card} />;
  if (record.source.kind === "codex")
    return <CodexListQuota record={record} card={card} />;
  return (
    <SubscriptionAccountQuota
      provider={record.source.provider}
      id={record.source.accountId}
      login={record.name}
      ready={!!record.source.accountId && record.reauthRequired !== true}
      disabled={false}
      compact
      card={card}
    />
  );
}
