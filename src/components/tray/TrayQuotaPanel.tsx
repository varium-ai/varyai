import { useCallback, useState } from "react";
import { useIsFetching, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { subscriptionPlanLabel } from "@/lib/subscriptionPlanLabel";
import { apiProviderQuotaEnabled } from "@/lib/query/apiProviderQuota";
import { ProviderListQuota } from "@/components/providers/ProviderListQuota";
import {
  UpstreamIcon,
  upstreamIcon,
} from "@/components/providers/UpstreamIcon";

export function TrayQuotaPanel() {
  const { t } = useTranslation();
  const [groupCount, setGroupCount] = useState(0);
  const countGroup = useCallback((node: HTMLElement | null) => {
    setGroupCount((count) => count + (node ? 1 : -1));
  }, []);
  const fetching = useIsFetching({
    predicate: (query) =>
      [
        "routing-provider-quota",
        "provider-list-codex-accounts",
        "codex-provider-quota",
        "subscription-account-quota",
      ].includes(String(query.queryKey[0])),
  });
  const [remaining, setRemaining] = useState(
    () => localStorage.getItem("vary:tray-quota-left") !== "false",
  );
  const providers = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    staleTime: 60_000,
    retry: false,
  });
  const records = (providers.data ?? []).filter(
    (record) => record.source.kind !== "api" || apiProviderQuotaEnabled(record),
  );
  const card = {
    remaining,
    toggle: () =>
      setRemaining((value) => {
        localStorage.setItem("vary:tray-quota-left", String(!value));
        return !value;
      }),
  };
  return (
    <div className="tray-quotas">
      {(providers.isPending || (!groupCount && fetching > 0)) && (
        <p role="status">{t("codexProvider.quotaLoading")}</p>
      )}
      {providers.isError && <p role="alert">{t("trayPanel.quotaFailed")}</p>}
      {!providers.isPending &&
        !providers.isError &&
        !groupCount &&
        !fetching && <p className="tray-empty">{t("trayPanel.quotaEmpty")}</p>}
      {records.map((record) => (
        <ProviderListQuota
          key={record.id}
          record={record}
          card={{
            ...card,
            renderGroup: (content) => (
              <section
                className="tray-quota-group"
                aria-label={record.name}
                ref={countGroup}
              >
                <header className="tray-quota-heading">
                  <span aria-hidden="true">
                    <UpstreamIcon
                      icon={upstreamIcon(record)}
                      name={record.name}
                      size={14}
                    />
                  </span>
                  <h2 title={record.name}>{record.name}</h2>
                  <span className="tray-quota-plan">
                    {record.subscriptionPlans
                      ?.map(subscriptionPlanLabel)
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                  <button
                    type="button"
                    onClick={card.toggle}
                    aria-label={t(
                      remaining
                        ? "trayPanel.showUsed"
                        : "trayPanel.showRemaining",
                    )}
                  >
                    {t(
                      remaining
                        ? "trayPanel.quotaRemaining"
                        : "trayPanel.quotaUsed",
                    )}
                  </button>
                </header>
                {content}
              </section>
            ),
          }}
        />
      ))}
    </div>
  );
}
