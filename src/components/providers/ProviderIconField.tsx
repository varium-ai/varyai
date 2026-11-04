import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { iconList, getIconMetadata } from "@/icons/extracted";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { UpstreamIcon } from "./UpstreamIcon";

const MAX_FILE = 1024 * 1024;
export function ProviderIconField({
  draft,
  icon,
  disabled,
  onChange,
  onBusyChange,
}: {
  draft: RoutingUpstream;
  icon?: string;
  disabled: boolean;
  onChange: (value: RoutingUpstream) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const fileInput = useRef<HTMLInputElement>(null);
  const [showBuiltin, setShowBuiltin] = useState(false);
  const [showUrl, setShowUrl] = useState(false);
  const [query, setQuery] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const blocked = disabled || busy;
  function choose(icon: string | null) {
    onChange({ ...draft, icon });
    setError("");
  }
  function pending(value: boolean) {
    setBusy(value);
    onBusyChange(value);
  }
  async function fetchIcon(automatic: boolean) {
    const address =
      automatic && draft.source.kind === "api"
        ? draft.source.baseUrl
        : url.trim();
    if (!address) return;
    pending(true);
    setError("");
    try {
      choose(await routingUpstreamsApi.fetchIcon(address, automatic));
    } catch {
      setError("providerEditor.iconFailed");
    } finally {
      pending(false);
    }
  }
  async function upload(file?: File) {
    if (!file) return;
    if (
      !file.size ||
      file.size > MAX_FILE ||
      !/\.(png|jpe?g|gif|webp|ico|svg)$/i.test(file.name)
    ) {
      setError("providerEditor.iconFileHint");
      return;
    }
    pending(true);
    setError("");
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () =>
          typeof reader.result === "string" ? resolve(reader.result) : reject();
        reader.onerror = reject;
        reader.readAsDataURL(file);
      });
      const image = new Image();
      await new Promise<void>((resolve, reject) => {
        image.onload = () => resolve();
        image.onerror = reject;
        image.src = data;
      });
      const ratio = Math.min(
        1,
        256 / Math.max(image.naturalWidth, image.naturalHeight),
      );
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(image.naturalWidth * ratio));
      canvas.height = Math.max(1, Math.round(image.naturalHeight * ratio));
      const context = canvas.getContext("2d");
      if (!context) throw new Error();
      context.drawImage(image, 0, 0, canvas.width, canvas.height);
      const png = canvas.toDataURL("image/png");
      const artwork =
        png.length <= 174800 ? png : canvas.toDataURL("image/webp", 0.85);
      if (artwork.length > 174800) throw new Error();
      choose(artwork);
    } catch {
      setError("providerEditor.iconFailed");
    } finally {
      pending(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  }
  return (
    <div className="pv-field">
      <span className="pv-field-label" id="provider-icon-label">
        {t("providerEditor.icon")}
      </span>
      <div className="pv-stack">
        <div
          className="pv-icon-controls"
          role="group"
          aria-labelledby="provider-icon-label"
        >
          <span className="pv-icon-preview" aria-hidden>
            <UpstreamIcon icon={icon} name={draft.name} size={24} />
          </span>
          <input
            ref={fileInput}
            hidden
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp,image/x-icon,image/svg+xml,.ico,.svg"
            aria-label={t("providerEditor.choosePicture")}
            disabled={blocked}
            onChange={(event) => void upload(event.target.files?.[0])}
          />
          <Button
            type="button"
            variant="ghost"
            disabled={blocked}
            onClick={() => fileInput.current?.click()}
          >
            {t("providerEditor.choosePicture")}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button type="button" variant="ghost" disabled={blocked}>
                {t(
                  busy
                    ? "providerEditor.lookingForIcon"
                    : "providerEditor.getIcon",
                )}
                <ChevronDown size={12} />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="pv-icon-menu" align="start">
              <DropdownMenuItem
                disabled={draft.source.kind !== "api" || !draft.source.baseUrl}
                onSelect={() => void fetchIcon(true)}
              >
                {t("providerEditor.automaticIcon")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setShowUrl(true)}>
                {t("providerEditor.imageUrl")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => choose("letter")}>
                {t("providerEditor.letterIcon")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            type="button"
            variant="ghost"
            disabled={blocked}
            aria-expanded={showBuiltin}
            onClick={() => setShowBuiltin(!showBuiltin)}
          >
            {t("providerEditor.builtinIcons")}
          </Button>
          {draft.icon && (
            <Button
              type="button"
              variant="ghost"
              disabled={blocked}
              onClick={() => choose(null)}
            >
              {t("providerEditor.defaultIcon")}
            </Button>
          )}
        </div>
        {showUrl && (
          <div className="pv-icon-url">
            <Input
              type="url"
              aria-label={t("providerEditor.imageUrl")}
              placeholder="https://example.com/icon.png"
              value={url}
              disabled={blocked}
              onChange={(event) => setUrl(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void fetchIcon(false);
                }
              }}
            />
            <Button
              type="button"
              variant="outline"
              disabled={blocked || !url.trim()}
              onClick={() => void fetchIcon(false)}
            >
              {t("providerEditor.useIcon")}
            </Button>
          </div>
        )}
        {showBuiltin && (
          <div className="pv-icon-picker">
            <Input
              type="search"
              aria-label={t("iconPicker.search")}
              placeholder={t("iconPicker.searchPlaceholder")}
              value={query}
              disabled={blocked}
              onChange={(event) => setQuery(event.target.value)}
            />
            <div className="pv-icon-grid">
              {iconList
                .filter((name) =>
                  `${name} ${getIconMetadata(name)?.displayName ?? ""}`
                    .toLocaleLowerCase()
                    .includes(query.trim().toLocaleLowerCase()),
                )
                .map((name) => (
                  <button
                    key={name}
                    type="button"
                    aria-label={getIconMetadata(name)?.displayName ?? name}
                    title={getIconMetadata(name)?.displayName ?? name}
                    aria-pressed={draft.icon === name}
                    disabled={blocked}
                    onClick={() => {
                      choose(name);
                      setShowBuiltin(false);
                    }}
                  >
                    <UpstreamIcon icon={name} name={name} size={24} />
                  </button>
                ))}
            </div>
          </div>
        )}
        {error && (
          <p role="alert" className="pv-icon-error">
            {t(error)}
          </p>
        )}
      </div>
    </div>
  );
}
