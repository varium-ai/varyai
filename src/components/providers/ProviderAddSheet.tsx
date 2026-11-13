import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  Box,
  ChevronRight,
  Download,
  Search,
  SlidersHorizontal,
  UserRound,
} from "lucide-react";
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

import {
  ProviderPickerDialog,
  type ProviderGroup,
} from "./ProviderPickerDialog";

export function ProviderAddSheet({
  onChoose,
  onClose,
  onImport,
  onAccount,
  busy,
  addedSubscriptions = [],
}: {
  onChoose: (preset?: GlobalProviderPreset) => void;
  onClose: () => void;
  onImport: () => void;
  onAccount: (
    provider: SubscriptionProviderId,
    accountId: string,
  ) => Promise<void>;
  addedSubscriptions?: SubscriptionProviderId[];
  busy: boolean;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [picker, setPicker] = useState<{ group: ProviderGroup; open: boolean }>(
    { group: "subscription", open: false },
  );
  const pickerTrigger = useRef<HTMLButtonElement | null>(null);
  const [signing, setSigning] = useState<{
    provider: SubscriptionProviderId;
    attempt: number;
  }>();
  function chooseGroup(group: ProviderGroup) {
    setSigning(undefined);
    setPicker({ group, open: true });
  }
  function back() {
    setSigning(undefined);
    onClose();
  }
  const term = query.trim().toLocaleLowerCase();
  const matchingVendors = globalProviderPresets.filter(
    (p) =>
      p.kind === "vendor" &&
      `${p.id} ${p.name} ${p.baseUrl} ${Object.values(p.china?.endpoints ?? {}).join(" ")} ${p.regions?.map((region) => `${region.name} ${Object.values(region.endpoints).join(" ")}`).join(" ") ?? ""}`
        .toLocaleLowerCase()
        .includes(term),
  );
  const matchingSubscriptions = subscriptionProviders.filter(
    (provider) =>
      provider.id === signing?.provider ||
      `${provider.name} ${provider.aliases}`.toLocaleLowerCase().includes(term),
  );
  const popular = !term;
  const matches = popular ? matchingVendors.slice(0, 4) : matchingVendors;
  const subscriptions = popular
    ? matchingSubscriptions.filter((provider) =>
        ["codex", "claude", "gemini_cli", "antigravity"].includes(provider.id),
      )
    : matchingSubscriptions;
  const methods = [
    {
      id: "subscription",
      title: "providerParity.subscription",
      description: "providerFlow.subscriptionDescription",
      icon: UserRound,
      action: () => chooseGroup("subscription"),
    },
    {
      id: "vendor",
      title: "providerFlow.vendor",
      description: "providerFlow.vendorDescription",
      icon: Box,
      action: () => chooseGroup("vendor"),
    },
    {
      id: "import",
      title: "routingUpstreams.import",
      description: "providerFlow.importDescription",
      icon: Download,
      action: () => {
        setSigning(undefined);
        onImport();
      },
    },
    {
      id: "custom",
      title: "providerFlow.custom",
      description: "providerFlow.compatible",
      icon: SlidersHorizontal,
      action: () => {
        setSigning(undefined);
        onChoose();
      },
    },
  ];
  return (
    <section aria-label={t("providerFlow.sheet")} className="pv-sheet">
      <header className="pv-add-heading">
        <Button
          variant="outline"
          size="icon"
          className="pv-back"
          aria-label={t("common.back")}
          disabled={busy}
          onClick={back}
        >
          <ArrowLeft size={20} aria-hidden />
        </Button>
        <div className="pv-add-copy">
          <h1>{t("providerFlow.sheet")}</h1>
          <p>{t("providerFlow.chooseMethod")}</p>
        </div>
      </header>
      <div className="pv-add-methods">
        {methods.map(({ id, title, description, icon: Icon, action }) => (
          <button
            key={id}
            type="button"
            className="pv-add-method"
            aria-label={t(title)}
            aria-describedby={`pv-method-${id}-description`}
            disabled={busy}
            onClick={(event) => {
              pickerTrigger.current = event.currentTarget;
              action();
            }}
          >
            <span className="pv-method-icon">
              <Icon size={28} aria-hidden />
            </span>
            <span className="pv-method-copy">
              <strong>{t(title)}</strong>
              <span id={`pv-method-${id}-description`}>{t(description)}</span>
            </span>
            <ChevronRight size={20} aria-hidden />
          </button>
        ))}
      </div>
      <div className="pv-common-heading">
        <h2>{t("providerFlow.commonProviders")}</h2>
        <div className="pv-vendor-search">
          <Search size={14} aria-hidden />
          <Input
            type="search"
            autoFocus
            aria-label={t("providerFlow.findVendor")}
            placeholder={t("providerFlow.findVendor")}
            value={query}
            disabled={busy}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                if (query) setQuery("");
                else if (!busy) {
                  back();
                }
              }
            }}
          />
        </div>
      </div>
      <div className="pv-common-providers">
        {!!subscriptions.length && (
          <div className="pv-vendors">
            <h3>{t("providerParity.subscription")}</h3>
            <div className="pv-vendor-options">
              <div className={`pv-tiles${popular ? " pv-popular-tiles" : ""}`}>
                {subscriptions.map((provider) => (
                  <button
                    key={provider.id}
                    type="button"
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
                      size={22}
                    />
                    <span title={provider.name}>
                      {popular && provider.id === "codex"
                        ? "ChatGPT"
                        : provider.name}
                    </span>
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
          </div>
        )}
        {!!matches.length && (
          <div className="pv-vendors">
            <h3>{t("providerFlow.vendor")}</h3>
            <div className="pv-vendor-options">
              <div className={`pv-tiles${popular ? " pv-popular-tiles" : ""}`}>
                {matches.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    aria-label={p.name}
                    className="pv-tile"
                    disabled={busy}
                    onClick={() => {
                      setSigning(undefined);
                      onChoose(p);
                    }}
                  >
                    <ProviderIcon icon={p.icon} name={p.name} size={22} />
                    <span title={p.name}>
                      {popular && p.id === "google" ? "Google" : p.name}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
        {!matches.length && !subscriptions.length && (
          <p className="pv-empty">{t("providerFlow.noVendor")}</p>
        )}
      </div>
      <ProviderPickerDialog
        open={picker.open}
        group={picker.group}
        busy={busy}
        addedSubscriptions={addedSubscriptions}
        triggerRef={pickerTrigger}
        onClose={() => setPicker((previous) => ({ ...previous, open: false }))}
        onChoose={onChoose}
        onAccount={onAccount}
      />
    </section>
  );
}
