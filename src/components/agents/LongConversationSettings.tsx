import { useEffect, useId, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  intentRoutingApi,
  type RoutingContextSettings,
} from "@/lib/api/intentRouting";
import "./long-conversations.css";

function tokenText(tokens?: number) {
  if (!tokens) return "";
  if (tokens % 1000000 === 0) return `${tokens / 1000000}m`;
  if (tokens % 1000 === 0) return `${tokens / 1000}k`;
  return String(tokens);
}

export function LongConversationSettings({
  disabled = false,
  onBusyChange,
}: {
  disabled?: boolean;
  onBusyChange?: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const client = useQueryClient();
  const key = ["long-conversation-settings"];
  const policy = useQuery({
    queryKey: key,
    queryFn: intentRoutingApi.contextSettings,
  });
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const writing = useRef(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setText(tokenText(policy.data?.compactAt));
  }, [policy.data?.compactAt]);
  const unavailable =
    disabled || saving || policy.isPending || policy.isError || !policy.data;

  async function save(next: RoutingContextSettings) {
    if (unavailable || writing.current) return;
    writing.current = true;
    setSaving(true);
    onBusyChange?.(true);
    setError(null);
    try {
      await intentRoutingApi.setContextSettings(next);
      client.setQueryData(key, next);
      setText(tokenText(next.compactAt));
      await Promise.all(
        [
          "intent-routing-groups",
          "intent-routing-client",
          "agent-connections",
          "agent-model-catalog",
          "gateway-catalog",
        ].map((name) => client.invalidateQueries({ queryKey: [name] })),
      );
    } catch {
      setError("longConversations.failed");
    } finally {
      writing.current = false;
      setSaving(false);
      onBusyChange?.(false);
    }
  }
  function saveThreshold() {
    if (!policy.data || unavailable) return;
    const match = text.trim().match(/^(\d+(?:\.\d+)?)\s*([km]?)$/i);
    const n = match
      ? Number(match[1]) *
        ({ k: 1000, m: 1000000 }[match[2].toLowerCase()] ?? 1)
      : NaN;
    if (text.trim() && (!Number.isSafeInteger(n) || n < 1)) {
      setError("longConversations.invalid");
      return;
    }
    const compactAt = text.trim() ? n : undefined;
    if (compactAt === policy.data.compactAt) {
      setText(tokenText(compactAt));
      setError(null);
      return;
    }
    void save({
      fullWindow: policy.data.fullWindow,
      ...(compactAt ? { compactAt } : {}),
    });
  }
  return (
    <div className="mp-long-conversations" aria-labelledby={`${id}-title`}>
      <div className="mp-long-heading">
        <h3 id={`${id}-title`}>{t("longConversations.title")}</h3>
        <span>{t("longConversations.shared")}</span>
      </div>
      <div className="mp-long-controls">
        <div
          className="mp-long-segments"
          role="radiogroup"
          aria-label={t("longConversations.title")}
        >
          <button
            type="button"
            role="radio"
            aria-checked={!policy.data?.fullWindow}
            aria-label={t("longConversations.compact")}
            disabled={unavailable}
            onClick={() =>
              policy.data && void save({ ...policy.data, fullWindow: false })
            }
          >
            {t("longConversations.compactAt", {
              size: tokenText(policy.data?.compactAt ?? 272000).toUpperCase(),
            })}
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={!!policy.data?.fullWindow}
            disabled={unavailable}
            onClick={() =>
              policy.data && void save({ ...policy.data, fullWindow: true })
            }
          >
            {t("longConversations.fullWindow")}
          </button>
        </div>
        {!policy.data?.fullWindow && (
          <input
            type="text"
            className="mp-long-threshold"
            value={text}
            aria-label={t("longConversations.threshold")}
            placeholder="272k"
            disabled={unavailable}
            aria-describedby={`${id}-help`}
            aria-invalid={error === "longConversations.invalid"}
            onChange={(event) => {
              setText(event.target.value);
              setError(null);
            }}
            onBlur={saveThreshold}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                saveThreshold();
              }
            }}
          />
        )}
      </div>
      <p id={`${id}-help`}>{t("longConversations.help")}</p>
      {(error || policy.isError) && (
        <p role="alert">{t(error ?? "longConversations.failed")}</p>
      )}
    </div>
  );
}
