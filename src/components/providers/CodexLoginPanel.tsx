import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Copy, LoaderCircle } from "lucide-react";
import { Input } from "@/components/ui/input";

type Props = {
  mode: "browser" | "device";
  url?: string;
  code?: string;
  proxy: string;
  providerName?: string;
  waitingMessage?: string;
  starting?: boolean;
  submitting?: boolean;
  submitted?: boolean;
  error?: string;
  openFailed?: boolean;
  callbackBlocked?: boolean;
  onCancel: () => void;
  onReopen: () => void;
  onCopy: () => void;
  onDevice?: () => void;
  onImport?: () => void;
  onProxy?: (proxy: string) => void;
  onSubmit?: (url: string) => void;
  onCodeCopy?: () => void;
  onRetry?: () => void;
};
export function CodexLoginPanel(props: Props) {
  const { t } = useTranslation();
  const [callback, setCallback] = useState("");
  const [custom, setCustom] = useState(
    !!props.proxy && props.proxy !== "direct",
  );
  const [proxyUrl, setProxyUrl] = useState(
    props.proxy === "direct" ? "" : props.proxy,
  );
  const callbackLocked = props.submitting || props.submitted;
  const title = props.providerName
    ? t("subscriptionLogin.title", { name: props.providerName })
    : t("codexBrowser.title");
  const finish = () => {
    if (callback.trim() && !callbackLocked) props.onSubmit?.(callback.trim());
  };
  return (
    <div className="cx-signing" role="region" aria-label={title}>
      <LoaderCircle
        size={20}
        className={`cx-signing-spinner ${props.error ? "" : "animate-spin motion-reduce:animate-none"}`}
        aria-hidden
      />
      <div className="cx-signing-content">
        <div className="cx-signing-head">
          <b>{title}</b>
          <button type="button" onClick={props.onCancel}>
            {t("common.cancel")}
          </button>
        </div>
        <p className="cx-signing-note" role="status">
          {t(
            props.starting
              ? "codexBrowser.starting"
              : props.openFailed
                ? "codexBrowser.openFailed"
                : (props.waitingMessage ??
                  (props.mode === "device"
                    ? "codexBrowser.deviceHint"
                    : "codexBrowser.opened")),
          )}
        </p>
        {props.code && (
          <div className="cx-signing-code">
            <code>{props.code}</code>
            {props.onCodeCopy && (
              <button
                type="button"
                aria-label={t("subscriptionLogin.copyCode")}
                onClick={props.onCodeCopy}
              >
                <Copy size={14} />
              </button>
            )}
          </div>
        )}
        {props.url && (
          <div className="cx-signing-url">
            <Input
              readOnly
              value={props.url}
              aria-label={t("codexBrowser.authorizationUrl")}
              onFocus={(event) => event.target.select()}
            />
            <button
              type="button"
              aria-label={t("common.copy")}
              onClick={props.onCopy}
            >
              <Copy size={14} />
            </button>
          </div>
        )}
        <div className="cx-signing-actions">
          <button type="button" disabled={!props.url} onClick={props.onReopen}>
            {t("codexBrowser.reopen")}
          </button>
          {props.onProxy && (
            <label className="cx-signing-proxy">
              <span>{t("codexBrowser.proxy")}:</span>
              <span className="cx-signing-proxy-control">
                <select
                  aria-label={t("codexBrowser.proxy")}
                  value={
                    custom
                      ? "custom"
                      : props.proxy === "direct"
                        ? "direct"
                        : props.proxy
                          ? "custom"
                          : ""
                  }
                  onChange={(event) => {
                    if (event.target.value === "custom") setCustom(true);
                    else {
                      setCustom(false);
                      props.onProxy?.(event.target.value);
                    }
                  }}
                >
                  <option value="">{t("codexBrowser.globalProxy")}</option>
                  <option value="direct">{t("codexBrowser.direct")}</option>
                  <option value="custom">
                    {t("codexBrowser.customProxy")}
                  </option>
                </select>
                <ChevronDown size={12} aria-hidden />
              </span>
            </label>
          )}
          {props.onDevice && (
            <button type="button" onClick={props.onDevice}>
              {t(
                props.mode === "browser"
                  ? "codexBrowser.deviceWay"
                  : "codexBrowser.browserWay",
              )}
            </button>
          )}
          {props.onImport && (
            <button type="button" onClick={props.onImport}>
              {t("codexBrowser.importWay")}
            </button>
          )}
          {props.onRetry && (
            <button type="button" onClick={props.onRetry}>
              {t("common.retry")}
            </button>
          )}
        </div>
        {custom && props.onProxy && (
          <div className="cx-signing-callback">
            <Input
              value={proxyUrl}
              aria-label={t("codexBrowser.proxyUrl")}
              placeholder="http://127.0.0.1:7890"
              onChange={(event) => setProxyUrl(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  if (proxyUrl.trim()) props.onProxy?.(proxyUrl.trim());
                }
              }}
            />
            <button
              type="button"
              disabled={!proxyUrl.trim()}
              onClick={() => props.onProxy?.(proxyUrl.trim())}
            >
              {t("codexBrowser.applyProxy")}
            </button>
          </div>
        )}
        {props.mode === "browser" && props.onSubmit && (
          <>
            <p className="cx-signing-note">
              {t(
                props.callbackBlocked
                  ? "codexBrowser.portBlocked"
                  : "codexBrowser.callbackHint",
              )}
            </p>
            <div className="cx-signing-callback">
              <Input
                aria-label={t("codexBrowser.callbackUrl")}
                placeholder={t("codexBrowser.callbackUrl")}
                autoComplete="off"
                value={callback}
                disabled={callbackLocked}
                onChange={(event) => setCallback(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    finish();
                  }
                }}
              />
              <button
                type="button"
                className="mp-primary"
                disabled={!callback.trim() || callbackLocked || props.starting}
                onClick={finish}
              >
                {t(
                  props.submitting || props.submitted
                    ? "codexBrowser.finishing"
                    : "codexBrowser.finish",
                )}
              </button>
            </div>
          </>
        )}
        {props.error && (
          <p role="alert" className="pv-error">
            {t(props.error)}
          </p>
        )}
      </div>
    </div>
  );
}
