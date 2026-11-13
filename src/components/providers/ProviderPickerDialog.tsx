import { useEffect, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Search, X } from "lucide-react";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import {
  subscriptionProviders,
  type SubscriptionProviderId,
} from "@/config/subscriptionProviders";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SubscriptionSignIn } from "./SubscriptionSignIn";

export type ProviderGroup = "subscription" | "vendor";

export function ProviderPickerDialog({
  open,
  group,
  busy,
  addedSubscriptions,
  triggerRef,
  onClose,
  onChoose,
  onAccount,
}: {
  open: boolean;
  group: ProviderGroup;
  busy: boolean;
  addedSubscriptions: SubscriptionProviderId[];
  triggerRef: RefObject<HTMLButtonElement>;
  onClose: () => void;
  onChoose: (preset: GlobalProviderPreset) => void;
  onAccount: (provider: SubscriptionProviderId, id: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [signing, setSigning] = useState<{
    provider: SubscriptionProviderId;
    attempt: number;
  }>();
  useEffect(() => {
    if (open) setQuery("");
    else setSigning(undefined);
  }, [open]);
  const term = query.trim().toLocaleLowerCase();
  const vendors = globalProviderPresets.filter(
    (preset) =>
      group === "vendor" &&
      preset.kind === "vendor" &&
      `${preset.id} ${preset.name} ${preset.baseUrl} ${Object.values(preset.china?.endpoints ?? {}).join(" ")} ${preset.regions?.map((region) => `${region.name} ${Object.values(region.endpoints).join(" ")}`).join(" ") ?? ""}`
        .toLocaleLowerCase()
        .includes(term),
  );
  const subscriptions = subscriptionProviders.filter(
    (provider) =>
      group === "subscription" &&
      (provider.id === signing?.provider ||
        `${provider.name} ${provider.aliases}`
          .toLocaleLowerCase()
          .includes(term)),
  );
  function close() {
    setSigning(undefined);
    onClose();
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value && !busy) close();
      }}
    >
      <DialogContent
        className="vary-providers pv-sheet pv-picker-dialog"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (!document.querySelector('.pv-provider-dialog[data-state="open"]'))
            triggerRef.current?.focus({ preventScroll: true });
        }}
        onEscapeKeyDown={(event) => {
          if (busy || query) event.preventDefault();
          if (!busy && query) setQuery("");
        }}
        onInteractOutside={(event) => {
          if (busy) event.preventDefault();
        }}
      >
        <DialogHeader className="pv-picker-head">
          <DialogTitle>
            {t(
              group === "subscription"
                ? "providerParity.subscription"
                : "providerFlow.vendor",
            )}
          </DialogTitle>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="pv-picker-close"
            aria-label={t("common.close")}
            disabled={busy}
            onClick={close}
          >
            <X size={16} aria-hidden />
          </Button>
          <DialogDescription>
            {t(
              group === "subscription"
                ? "providerFlow.subscriptionDescription"
                : "providerFlow.vendorDescription",
            )}
          </DialogDescription>
          <div className="pv-vendor-search">
            <Search size={14} aria-hidden />
            <Input
              type="search"
              autoFocus
              aria-label={t("providerFlow.findVendor")}
              placeholder={t("providerFlow.findVendor")}
              value={query}
              disabled={busy}
              onChange={(event) => setQuery(event.target.value)}
            />
          </div>
        </DialogHeader>
        <DialogBody className="pv-picker-body">
          <div className="pv-vendors pv-directory">
            <div className="pv-vendor-options">
              <div className="pv-tiles">
                {subscriptions.map((provider) => (
                  <button
                    key={provider.id}
                    type="button"
                    className={`pv-tile${signing?.provider === provider.id ? " on" : ""}`}
                    aria-label={provider.name}
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
                    <span>{provider.name}</span>
                    {addedSubscriptions.includes(provider.id) && (
                      <span
                        className="pv-signed-in-dot"
                        aria-label={t("subscriptionLogin.signedIn")}
                      />
                    )}
                  </button>
                ))}
                {vendors.map((preset) => (
                  <button
                    key={preset.id}
                    type="button"
                    className="pv-tile"
                    aria-label={preset.name}
                    disabled={busy}
                    onClick={() => {
                      close();
                      onChoose(preset);
                    }}
                  >
                    <ProviderIcon
                      icon={preset.icon}
                      name={preset.name}
                      size={22}
                    />
                    <span>{preset.name}</span>
                  </button>
                ))}
              </div>
              {!subscriptions.length && !vendors.length && (
                <p className="pv-empty">{t("providerFlow.noVendor")}</p>
              )}
              {open && signing && (
                <SubscriptionSignIn
                  key={`${signing.provider}-${signing.attempt}`}
                  provider={signing.provider}
                  onCancel={() => setSigning(undefined)}
                  onComplete={(id) => {
                    const provider = signing.provider;
                    close();
                    void onAccount(provider, id);
                  }}
                />
              )}
            </div>
          </div>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
