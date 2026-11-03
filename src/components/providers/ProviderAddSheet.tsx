import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Search } from "lucide-react";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import { type RoutingUpstream } from "@/lib/api/routingUpstreams";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export function ProviderAddSheet({
  records,
  onChoose,
  onClose,
  onAccount,
  onImport,
  busy,
}: {
  records: RoutingUpstream[];
  onChoose: (preset?: GlobalProviderPreset) => void;
  onClose: () => void;
  onAccount: () => Promise<void>;
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
  return (
    <section aria-label={t("providerFlow.sheet")} className="pv-sheet">
      <div className="pv-sheet-head">
        <h2>
          {t(records.length ? "modelProviders.add" : "providerFlow.first")}
        </h2>
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
        <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>
      {(!term || "chatgpt codex openai".includes(term)) && (
        <div className="pv-vendors">
          <h3>{t("providerParity.subscription")}</h3>
          <button
            className="pv-tile"
            disabled={busy}
            onClick={() => void onAccount()}
          >
            <ProviderIcon icon="openai" name="ChatGPT" size={18} />
            <span>ChatGPT / Codex</span>
          </button>
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
      {!matches.length && (
        <p className="pv-empty">{t("providerFlow.noVendor")}</p>
      )}
      <div className="pv-custom">
        <Button variant="ghost" disabled={busy} onClick={() => onChoose()}>
          <Plus size={13} />
          {t("providerFlow.custom")}
        </Button>
        <span>{t("providerFlow.compatible")}</span>
      </div>
    </section>
  );
}
