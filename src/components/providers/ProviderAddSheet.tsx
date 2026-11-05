import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Plus, Search } from "lucide-react";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  subscriptionProviders,
  type SubscriptionProviderId,
} from "@/config/subscriptionProviders";

export function ProviderAddSheet({
  onChoose,
  onClose,
  onAccount,
  onImport,
  busy,
}: {
  onChoose: (preset?: GlobalProviderPreset) => void;
  onClose: () => void;
  onAccount: (provider: SubscriptionProviderId) => Promise<void>;
  onImport?: () => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const term = query.trim().toLocaleLowerCase();
  const matches = globalProviderPresets.filter((p) =>
    `${p.id} ${p.name} ${p.baseUrl} ${Object.values(p.china?.endpoints ?? {}).join(" ")} ${p.regions?.map((region) => `${region.name} ${Object.values(region.endpoints).join(" ")}`).join(" ") ?? ""}`
      .toLocaleLowerCase()
      .includes(term),
  );
  const subscriptions = subscriptionProviders.filter((provider) =>
    `${provider.name} ${provider.aliases}`.toLocaleLowerCase().includes(term),
  );
  return (
    <section aria-label={t("providerFlow.sheet")} className="pv-sheet">
      <header className="pv-add-heading">
        <Button
          variant="ghost"
          size="sm"
          className="pv-back"
          disabled={busy}
          onClick={onClose}
        >
          <ArrowLeft size={14} />
          {t("common.back")}
        </Button>
        <div className="pv-sheet-head">
          <div className="pv-vendor-search">
            <Search size={12} aria-hidden />
            <Input
              type="search"
              autoFocus
              aria-label={t("providerFlow.findVendor")}
              placeholder={t("providerFlow.findVendor")}
              value={query}
              disabled={busy}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  query ? setQuery("") : !busy && onClose();
                }
              }}
            />
          </div>
          <Button variant="ghost" size="sm" disabled={busy} onClick={onImport}>
            {t("providerFlow.import")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="pv-custom-action"
            title={t("providerFlow.compatible")}
            disabled={busy}
            onClick={() => onChoose()}
          >
            <Plus size={13} aria-hidden />
            {t("providerFlow.custom")}
          </Button>
        </div>
      </header>
      {!!subscriptions.length && (
        <div className="pv-vendors">
          <h3>
            {t("providerParity.subscription")}{" "}
            <span className="pv-subscription-hint">
              {t("subscriptionProvider.loginHint")}
            </span>
          </h3>
          <div className="pv-tiles">
            {subscriptions.map((provider) => (
              <button
                key={provider.id}
                aria-label={provider.name}
                className="pv-tile"
                disabled={busy}
                onClick={() => void onAccount(provider.id)}
              >
                <ProviderIcon
                  icon={provider.icon}
                  name={provider.name}
                  size={18}
                />
                <span>{provider.name}</span>
              </button>
            ))}
          </div>
        </div>
      )}
      {(["vendor", "relay"] as const).map((kind) => {
        const rows = matches.filter((p) => p.kind === kind);
        return (
          rows.length > 0 && (
            <div key={kind} className="pv-vendors">
              <h3>{t(`providerFlow.${kind}`)}</h3>
              <div className="pv-tiles">
                {rows.map((p) => (
                  <button
                    key={p.id}
                    aria-label={p.name}
                    className="pv-tile"
                    disabled={busy}
                    onClick={() => onChoose(p)}
                  >
                    <ProviderIcon icon={p.icon} name={p.name} size={18} />
                    <span>{p.name}</span>
                  </button>
                ))}
              </div>
            </div>
          )
        );
      })}
      {!matches.length && !subscriptions.length && (
        <p className="pv-empty">{t("providerFlow.noVendor")}</p>
      )}
    </section>
  );
}
