import { useLayoutEffect, useRef, useState } from "react";
import { Info } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { RoutingGroup } from "@/lib/api/intentRouting";

const firstPresets = [0, 30, 60, 120];
export function RoutingExecutionSettings({
  group,
  disabled,
  memberWindows = [],
  onChange,
}: {
  group: RoutingGroup;
  disabled: boolean;
  memberWindows?: number[];
  onChange: (group: RoutingGroup) => void;
}) {
  const { t } = useTranslation();
  const [custom, setCustom] = useState(
    !firstPresets.includes(group.firstToken ?? 0),
  );
  const [minutes, setMinutes] = useState(
    String(+((group.firstToken ?? 0) / 60).toFixed(2)),
  );
  const [legacy, setLegacy] = useState(false);
  const contextInput = useRef<HTMLInputElement>(null);
  const [contextCustom, setContextCustom] = useState(
    (group.contextWindow ?? 0) > 0,
  );
  const [tokens, setTokens] = useState(String(group.contextWindow ?? ""));
  const focusContext = useRef(false);
  useLayoutEffect(() => {
    if (contextCustom && focusContext.current) {
      contextInput.current?.focus({ preventScroll: true });
      contextInput.current?.select();
      focusContext.current = false;
    }
  }, [contextCustom]);
  const customInput = useRef<HTMLInputElement>(null);
  const focusCustom = useRef(false);
  useLayoutEffect(() => {
    if (custom && focusCustom.current) {
      customInput.current?.focus({ preventScroll: true });
      customInput.current?.select();
      focusCustom.current = false;
    }
  }, [custom]);
  const info = (text: string) => (
    <span
      className="rt-info"
      role="img"
      tabIndex={0}
      aria-label={text}
      title={text}
    >
      <Info size={14} />
    </span>
  );
  return (
    <div className="rt-execution-picks">
      <div className="rt-setting-row">
        <span>{t("routingPage.slowToStart")}</span>
        <div className="rt-setting-content">
          <div
            className="rt-reference-seg"
            role="radiogroup"
            aria-label={t("routingPage.slowToStart")}
          >
            {[...firstPresets, "custom"].map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                disabled={disabled}
                aria-checked={
                  custom
                    ? value === "custom"
                    : value === (group.firstToken ?? 0)
                }
                onClick={() => {
                  setCustom(value === "custom");
                  if (value === "custom") {
                    focusCustom.current = !custom;
                    const seconds =
                      (group.firstToken ?? 0) > 0 ? group.firstToken! : 300;
                    setMinutes(String(+(seconds / 60).toFixed(2)));
                    onChange({ ...group, firstToken: seconds });
                  } else onChange({ ...group, firstToken: Number(value) });
                }}
              >
                {value === "custom"
                  ? t("routingPage.customTimeout")
                  : value === 0
                    ? t("routingPage.waitForStart")
                    : value === 30
                      ? "30 s"
                      : value === 60
                        ? "1 min"
                        : "2 min"}
              </button>
            ))}
          </div>
          {custom && (
            <label className="rt-timeout-custom">
              <input
                ref={customInput}
                type="text"
                inputMode="decimal"
                disabled={disabled}
                aria-label={t("routingPage.timeoutMinutes")}
                aria-invalid={
                  !Number.isInteger(group.firstToken) ||
                  (group.firstToken ?? 0) <= 0 ||
                  (group.firstToken ?? 0) > 3600
                }
                value={minutes}
                onChange={(event) => {
                  const text = event.target.value;
                  setMinutes(text);
                  const normalized = text.trim().replace(",", ".");
                  const seconds = /^\d+(?:\.\d+)?$/.test(normalized)
                    ? Math.round(Number(normalized) * 60)
                    : NaN;
                  onChange({ ...group, firstToken: seconds });
                }}
              />
              <span>min</span>
            </label>
          )}
          {info(t("intentRouting.firstTokenHelp"))}
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("routingPage.everyMemberBusy")}</span>
        <div className="rt-setting-content">
          <div
            className="rt-reference-seg"
            role="radiogroup"
            aria-label={t("routingPage.everyMemberBusy")}
          >
            {[
              [-1, "patienceOff"],
              [0, "patienceOneMinute"],
              [120, "patienceTwoMinutes"],
              [240, "patienceFourMinutes"],
            ].map(([value, key]) => (
              <button
                key={value}
                type="button"
                role="radio"
                disabled={disabled}
                aria-checked={(group.patience ?? -1) === value}
                onClick={() => onChange({ ...group, patience: Number(value) })}
              >
                {t(`routingPage.${key}`)}
              </button>
            ))}
          </div>
          {![undefined, -1, 0, 120, 240].includes(group.patience) && (
            <span className="rt-setting-inherited">{group.patience} s</span>
          )}
          {info(t("intentRouting.patienceHelp"))}
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("intentRouting.contextPolicy")}</span>
        <div className="rt-setting-content">
          <div
            className="rt-reference-seg"
            role="radiogroup"
            aria-label={t("intentRouting.contextPolicy")}
          >
            {(["largest", "smallest", "custom"] as const).map((mode) => (
              <button
                type="button"
                role="radio"
                key={mode}
                disabled={disabled}
                aria-checked={
                  contextCustom
                    ? mode === "custom"
                    : group.contextWindow === -1
                      ? mode === "smallest"
                      : mode === "largest"
                }
                onClick={() => {
                  setContextCustom(mode === "custom");
                  if (mode === "custom") {
                    const known = memberWindows.filter(
                      (n) => Number.isSafeInteger(n) && n >= 2048,
                    );
                    const window =
                      (group.contextWindow ?? 0) > 0
                        ? group.contextWindow!
                        : known.length
                          ? Math.min(...known)
                          : 200000;
                    setTokens(String(window));
                    focusContext.current = !contextCustom;
                    onChange({ ...group, contextWindow: window });
                  } else
                    onChange({
                      ...group,
                      contextWindow: mode === "smallest" ? -1 : undefined,
                    });
                }}
              >
                {t(
                  `intentRouting.context${mode[0].toUpperCase()}${mode.slice(1)}`,
                )}
              </button>
            ))}
          </div>
          {contextCustom && (
            <input
              ref={contextInput}
              className="rt-context-custom"
              type="text"
              inputMode="decimal"
              aria-label={t("intentRouting.contextTokens")}
              aria-invalid={
                !Number.isSafeInteger(group.contextWindow) ||
                (group.contextWindow ?? 0) < 2048
              }
              disabled={disabled}
              value={tokens}
              onChange={(event) => {
                setTokens(event.target.value);
                const match = /^(\d+(?:\.\d+)?)\s*([km]?)$/i.exec(
                  event.target.value.trim().replace(/_/g, ""),
                );
                const count = match
                  ? Math.round(
                      Number(match[1]) *
                        (match[2].toLowerCase() === "m"
                          ? 1e6
                          : match[2].toLowerCase() === "k"
                            ? 1e3
                            : 1),
                    )
                  : NaN;
                onChange({
                  ...group,
                  contextWindow:
                    Number.isSafeInteger(count) && count >= 2048 ? count : NaN,
                });
              }}
            />
          )}
          {info(t("intentRouting.contextHelp"))}
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("routingPage.stays")}</span>
        <div className="rt-setting-content">
          <div
            className="rt-reference-seg"
            role="radiogroup"
            aria-label={t("intentRouting.affinity")}
          >
            {(["session", "turn", "off", "strict"] as const).map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                disabled={disabled}
                aria-checked={(group.affinity || "session") === value}
                onClick={() => onChange({ ...group, affinity: value })}
              >
                {t(`intentRouting.affinityOptions.${value}`)}
              </button>
            ))}
          </div>
          {info(t("intentRouting.affinityHelp"))}
        </div>
      </div>
      <details
        className="rt-execution-legacy"
        open={legacy}
        onToggle={(event) => setLegacy(event.currentTarget.open)}
      >
        <summary>{t("routingPage.executionAdvanced")}</summary>
        <div className="rt-execution-settings">
          <label>
            <span>{t("intentRouting.firstToken")}</span>
            <input
              type="number"
              min={0}
              max={3600}
              disabled={disabled}
              aria-label={t("intentRouting.firstToken")}
              value={
                Number.isFinite(group.firstToken ?? 0)
                  ? (group.firstToken ?? 0)
                  : ""
              }
              onChange={(event) => {
                const seconds =
                  event.target.value === "" ? NaN : Number(event.target.value);
                setCustom(!firstPresets.includes(seconds));
                setMinutes(String(+(seconds / 60).toFixed(2)));
                onChange({ ...group, firstToken: seconds });
              }}
            />
          </label>
          <label>
            <span>{t("intentRouting.patience")}</span>
            <input
              type="number"
              min={-1}
              max={240}
              disabled={disabled}
              aria-label={t("intentRouting.patience")}
              value={
                Number.isFinite(group.patience ?? -1)
                  ? (group.patience ?? -1)
                  : ""
              }
              onChange={(event) =>
                onChange({
                  ...group,
                  patience:
                    event.target.value === ""
                      ? NaN
                      : Number(event.target.value),
                })
              }
            />
          </label>
        </div>
      </details>
    </div>
  );
}
