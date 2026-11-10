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
import { SubscriptionSignIn } from "./SubscriptionSignIn";

export function ProviderAddSheet({
  onChoose,
  onClose,
  onAccount,
  busy,
  addedSubscriptions = [],
}: {
  onChoose: (preset?: GlobalProviderPreset) => void;
  onClose: () => void;
  onAccount: (
    provider: SubscriptionProviderId,
    accountId: string,
  ) => Promise<void>;
  addedSubscriptions?: SubscriptionProviderId[];
  busy: boolean;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [signing, setSigning] = useState<{
    provider: SubscriptionProviderId;
    attempt: number;
  }>();
  const term = query.trim().toLocaleLowerCase();
  const matches = globalProviderPresets.filter((p) =>
    `${p.id} ${p.name} ${p.baseUrl} ${Object.values(p.china?.endpoints ?? {}).join(" ")} ${p.regions?.map((region) => `${region.name} ${Object.values(region.endpoints).join(" ")}`).join(" ") ?? ""}`
      .toLocaleLowerCase()
      .includes(term),
  );
  const subscriptions = subscriptionProviders.filter(
    (provider) =>
      provider.id === signing?.provider ||
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
          onClick={() => {
            setSigning(undefined);
            onClose();
          }}
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
                  if (query) setQuery("");
                  else if (!busy) {
                    setSigning(undefined);
                    onClose();
                  }
                }
              }}
            />
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="pv-custom-action"
            title={t("providerFlow.compatible")}
            disabled={busy}
            onClick={() => {
              setSigning(undefined);
              onChoose();
            }}
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
                className={`pv-tile${signing?.provider === provider.id ? " on" : ""}`}
                aria-pressed={signing?.provider === provider.id}
                disabled={busy}
                onClick={() =>
                  setSigning((old) => ({
                    provider: provider.id,
                    attempt: (old?.attempt ?? 0) + 1,
                  }))
                }
              >
                <ProviderIcon
                  icon={provider.icon}
                  name={provider.name}
                  size={18}
                />
                <span>{provider.name}</span>
                {addedSubscriptions.includes(provider.id) && (
                  <span
                    className="pv-signed-in-dot"
                    aria-label={t("subscriptionLogin.signedIn")}
                  />
                )}
              </button>
            ))}
          </div>
          {signing && (
            <SubscriptionSignIn
              key={`${signing.provider}-${signing.attempt}`}
              provider={signing.provider}
              onCancel={() => setSigning(undefined)}
              onComplete={(id) => {
                void onAccount(signing.provider, id);
              }}
            />
          )}
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
                    onClick={() => {
                      setSigning(undefined);
                      onChoose(p);
                    }}
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
