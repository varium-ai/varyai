import { useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import {
  apiProviderQuotaEnabled,
  apiProviderQuotaOptions,
} from "@/lib/query/apiProviderQuota";
import { ApiProviderQuotaView } from "@/components/providers/ApiProviderQuota";
import { ProviderListQuota } from "@/components/providers/ProviderListQuota";
import {
  UpstreamIcon,
  upstreamIcon,
} from "@/components/providers/UpstreamIcon";

export function TrayQuotaPanel() {
  const { t } = useTranslation();
  const client = useQueryClient();
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
  const relays = records.filter((record) => record.source.kind === "api");
  // Keep observers mounted when an unsupported relay's card is hidden, so a
  // later automatic or explicit refresh can discover its balance interface.
  const relayQueries = useQueries({
    queries: relays.map((record) =>
      apiProviderQuotaOptions(record, client, true),
    ),
  });
  const relayObservations = new Map(
    relays.map((record, index) => [record.id, relayQueries[index]]),
  );
  const visible = records.filter(
    (record) =>
      relayObservations.get(record.id)?.data?.quota?.status !== "unsupported",
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
      {providers.isPending && (
        <p role="status">{t("codexProvider.quotaLoading")}</p>
      )}
      {providers.isError && <p role="alert">{t("trayPanel.quotaFailed")}</p>}
      {!providers.isPending && !providers.isError && !visible.length && (
        <p className="tray-empty">{t("trayPanel.quotaEmpty")}</p>
      )}
      {visible.map((record) => {
        const observation = relayObservations.get(record.id);
        return (
          <section
            className="tray-quota-group"
            key={record.id}
            aria-label={record.name}
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
                {record.subscriptionPlans?.filter(Boolean).join(" · ")}
              </span>
              <button
                type="button"
                onClick={card.toggle}
                aria-label={t(
                  remaining ? "trayPanel.showUsed" : "trayPanel.showRemaining",
                )}
              >
                {t(
                  remaining
                    ? "trayPanel.quotaRemaining"
                    : "trayPanel.quotaUsed",
                )}
              </button>
            </header>
            {observation ? (
              <ApiProviderQuotaView
                record={record}
                card={card}
                observation={observation}
              />
            ) : (
              <ProviderListQuota record={record} card={card} />
            )}
          </section>
        );
      })}
    </div>
  );
}
