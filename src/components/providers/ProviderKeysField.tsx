import { useEffect, useId, useState, type RefObject } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Check, Copy, Pencil, Plus, Trash2 } from "lucide-react";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { copyText } from "@/lib/clipboard";

function mask(value: string) {
  const chars = Array.from(value);
  return chars.length < 12
    ? "••••••••"
    : `${chars.slice(0, 4).join("")}…${chars.slice(-4).join("")}`;
}

export function ProviderKeysField({
  draft,
  apiKey,
  disabled,
  onDraftChange,
  onKeyChange,
  onBusyChange,
  inputRef,
}: {
  draft: RoutingUpstream;
  apiKey: string;
  disabled: boolean;
  onDraftChange: (draft: RoutingUpstream) => void;
  onKeyChange: (value: string) => void;
  onBusyChange: (value: boolean) => void;
  inputRef?: RefObject<HTMLInputElement>;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [editing, setEditing] = useState<string | null>(
    draft.hasApiKey && draft.revision ? null : "",
  );
  const [show, setShow] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    onBusyChange(copying);
  }, [copying, onBusyChange]);
  const summaries = useQuery({
    queryKey: ["routing-upstream-keys", draft.id, draft.revision],
    queryFn: () => routingUpstreamsApi.keys(draft.id, draft.revision),
    enabled: !!draft.revision && typeof routingUpstreamsApi.keys === "function",
    refetchInterval: 3000,
    retry: false,
  });
  if (draft.source.kind !== "api") return null;
  const transport = draft.transport ?? {
    endpoints: {},
    fullUrl: false,
    keys: [],
  };
  const keys = [
    {
      id: "",
      name: t("providerEditor.primaryKey"),
      enabled: !transport.primaryOff,
      key: apiKey,
    },
    ...transport.keys,
  ];
  const preferred = transport.preferredKey ?? "";
  const blocked = disabled || copying;
  const current = keys.find((key) => key.id === editing);
  function updateTransport(next: typeof transport) {
    onDraftChange({ ...draft, transport: next });
  }
  function toggle(keyId: string, enabled: boolean) {
    updateTransport(
      keyId === ""
        ? { ...transport, primaryOff: !enabled }
        : {
            ...transport,
            keys: transport.keys.map((key) =>
              key.id === keyId ? { ...key, enabled } : key,
            ),
          },
    );
  }
  function prefer(keyId: string) {
    updateTransport({
      ...transport,
      preferredKey: keyId || null,
      ...(keyId === ""
        ? { primaryOff: false }
        : {
            keys: transport.keys.map((key) =>
              key.id === keyId ? { ...key, enabled: true } : key,
            ),
          }),
    });
  }
  function edit(keyId: string) {
    setEditing(keyId);
    setShow(false);
    setError(false);
  }
  function add() {
    const keyId = crypto.randomUUID();
    updateTransport({
      ...transport,
      keys: [
        ...transport.keys,
        { id: keyId, name: "", enabled: true, key: "" },
      ],
    });
    edit(keyId);
  }
  function changeKey(value: string) {
    if (editing === "") onKeyChange(value);
    else
      updateTransport({
        ...transport,
        keys: transport.keys.map((key) =>
          key.id === editing ? { ...key, key: value || undefined } : key,
        ),
      });
  }
  function remove() {
    updateTransport({
      ...transport,
      preferredKey: preferred === editing ? null : transport.preferredKey,
      keys: transport.keys.filter((key) => key.id !== editing),
    });
    setEditing(null);
  }
  async function copy(keyId: string) {
    setCopying(true);
    setError(false);
    setCopied(null);
    try {
      const value =
        keys.find((key) => key.id === keyId)?.key ||
        (await routingUpstreamsApi.revealKey(draft.id, draft.revision, keyId));
      await copyText(value);
      setCopied(keyId);
    } catch {
      setError(true);
    } finally {
      setCopying(false);
    }
  }
  return (
    <div className="pv-field">
      <span className="pv-field-label" id={`${id}-label`}>
        {t("providerEditor.accounts")}
      </span>
      <div className="pv-stack">
        <div
          className="pv-account-list pv-key-pool"
          role="group"
          aria-label={t("providerEditor.savedKeys", { count: keys.length })}
        >
          <ul className="pv-saved-keys">
            {keys.map((key, index) => {
              const summary = summaries.data?.find(
                (entry) => entry.id === key.id,
              );
              const name =
                key.name || t("providerEditor.additionalKey", { n: index });
              return (
                <li
                  className={`pv-account pv-key-row ${key.enabled ? "" : "off"}`}
                  key={key.id}
                >
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={key.enabled}
                    aria-label={t("providerEditor.useKey", { name })}
                    className="pv-key-check"
                    disabled={blocked}
                    onClick={() => toggle(key.id, !key.enabled)}
                  >
                    <Check size={13} aria-hidden="true" />
                  </button>
                  <span className="pv-key-label" title={name}>
                    {summary?.label && !key.key
                      ? summary.label
                      : key.key
                        ? mask(key.key)
                        : name}
                  </span>
                  <button
                    type="button"
                    className="pv-key-action"
                    aria-label={t("providerEditor.copyKey")}
                    title={t("providerEditor.copyKey")}
                    disabled={blocked || (!key.key && !summary)}
                    onClick={() => void copy(key.id)}
                  >
                    {copied === key.id ? (
                      <Check size={12} />
                    ) : (
                      <Copy size={12} />
                    )}
                  </button>
                  <button
                    type="button"
                    className="pv-key-action"
                    aria-label={t("providerEditor.editKey")}
                    title={t("providerEditor.editKey")}
                    disabled={blocked}
                    onClick={() => edit(key.id)}
                  >
                    <Pencil size={12} />
                  </button>
                  {summary?.running != null && summary.running > 0 && (
                    <span className="pv-key-running">
                      {t("providerEditor.keyRunning", {
                        count: summary.running,
                      })}
                    </span>
                  )}
                  {preferred === key.id ? (
                    <small>{t("providerEditor.preferredKey")}</small>
                  ) : (
                    <button
                      type="button"
                      className="pv-key-prefer"
                      disabled={blocked}
                      onClick={() => prefer(key.id)}
                    >
                      {t("providerEditor.makePreferred")}
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
          <button
            type="button"
            className="pv-key-add"
            disabled={blocked || transport.keys.length >= 64}
            onClick={add}
          >
            <Plus size={16} />
            {t("providerEditor.addKey")}
          </button>
        </div>
        <p className="pv-hint">{t("providerEditor.keyPoolHint")}</p>
        {current && (
          <div className="pv-key-edit pv-stack">
            {editing !== "" && (
              <Input
                aria-label={t("providerEditor.keyName")}
                value={current.name}
                placeholder={t("providerEditor.keyName")}
                disabled={blocked}
                onChange={(event) =>
                  updateTransport({
                    ...transport,
                    keys: transport.keys.map((key) =>
                      key.id === editing
                        ? { ...key, name: event.target.value }
                        : key,
                    ),
                  })
                }
              />
            )}
            <div className="pv-key-input">
              <label htmlFor={`${id}-key`} className="sr-only">
                {t("modelProviders.apiKey")}
              </label>
              <Input
                ref={inputRef}
                id={`${id}-key`}
                type={show ? "text" : "password"}
                autoComplete="new-password"
                value={current.key ?? ""}
                required={
                  editing === ""
                    ? !draft.hasApiKey
                    : !summaries.data?.some((entry) => entry.id === editing)
                }
                disabled={blocked}
                onChange={(event) => changeKey(event.target.value)}
                placeholder={
                  draft.hasApiKey
                    ? t("modelProviders.keepKey")
                    : t("providerEditor.keyPlaceholder")
                }
              />
              {!!current.key && (
                <Button
                  type="button"
                  variant="ghost"
                  disabled={blocked}
                  onClick={() => setShow(!show)}
                >
                  {t(
                    show ? "providerEditor.hideKey" : "providerEditor.showKey",
                  )}
                </Button>
              )}
              <Button
                type="button"
                variant="ghost"
                disabled={blocked}
                onClick={() => setEditing(null)}
              >
                {t("common.done")}
              </Button>
              {editing !== "" && (
                <Button
                  type="button"
                  variant="ghost"
                  aria-label={t("providerEditor.removeKey")}
                  disabled={blocked}
                  onClick={remove}
                >
                  <Trash2 size={13} />
                </Button>
              )}
            </div>
            <p className="pv-hint">{t("providerEditor.savedKeyHint")}</p>
          </div>
        )}
        {error && (
          <p role="alert" className="pv-error">
            {t("providerEditor.copyKeyFailed")}
          </p>
        )}
      </div>
    </div>
  );
}
