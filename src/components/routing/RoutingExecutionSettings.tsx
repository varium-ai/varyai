import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RoutingGroup } from "@/lib/api/intentRouting";
import { RoutingSegments } from "./RoutingSegments";

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
  return (
    <div className="rt-execution-picks">
      <div className="rt-setting-row">
        <span>{t("routingPage.slowToStart")}</span>
        <div className="rt-setting-content">
          <RoutingSegments
            label={t("routingPage.slowToStart")}
            value={custom ? "custom" : String(group.firstToken ?? 0)}
            disabled={disabled}
            options={[
              ...firstPresets.map((value) => ({
                value: String(value),
                label:
                  value === 0
                    ? t("routingPage.waitForStart")
                    : value === 30
                      ? "30 s"
                      : value === 60
                        ? "1 min"
                        : "2 min",
              })),
              { value: "custom", label: t("routingPage.customTimeout") },
            ]}
            onChange={(value) => {
              setCustom(value === "custom");
              if (value === "custom") {
                focusCustom.current = !custom;
                const seconds =
                  (group.firstToken ?? 0) > 0 ? group.firstToken! : 300;
                setMinutes(String(+(seconds / 60).toFixed(2)));
                onChange({ ...group, firstToken: seconds });
              } else onChange({ ...group, firstToken: Number(value) });
            }}
          />
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
          <p className="rt-field-hint">
            {t(
              (group.firstToken ?? 0) > 0
                ? "intentRouting.firstDeadlineHelp"
                : "intentRouting.firstWaitHelp",
              { seconds: group.firstToken },
            )}
          </p>
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("routingPage.everyMemberBusy")}</span>
        <div className="rt-setting-content">
          <RoutingSegments
            label={t("routingPage.everyMemberBusy")}
            value={String(group.patience ?? -1)}
            disabled={disabled}
            options={[
              [-1, "patienceOff"],
              [0, "patienceOneMinute"],
              [120, "patienceTwoMinutes"],
              [240, "patienceFourMinutes"],
            ].map(([value, key]) => ({
              value: String(value),
              label: t(`routingPage.${key}`),
            }))}
            onChange={(value) =>
              onChange({ ...group, patience: Number(value) })
            }
          />
          {![undefined, -1, 0, 120, 240].includes(group.patience) && (
            <span className="rt-setting-inherited">{group.patience} s</span>
          )}
          <p className="rt-field-hint">
            {t(
              (group.patience ?? -1) < 0
                ? "intentRouting.patienceOffHelp"
                : "intentRouting.patienceWaitHelp",
              { seconds: group.patience || 60 },
            )}
          </p>
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("intentRouting.contextPolicy")}</span>
        <div className="rt-setting-content">
          <RoutingSegments
            label={t("intentRouting.contextPolicy")}
            value={
              contextCustom
                ? "custom"
                : group.contextWindow === -1
                  ? "smallest"
                  : "largest"
            }
            disabled={disabled}
            options={(["largest", "smallest", "custom"] as const).map(
              (value) => ({
                value,
                label: t(
                  `intentRouting.context${value[0].toUpperCase()}${value.slice(1)}`,
                ),
              }),
            )}
            onChange={(mode) => {
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
          />
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
          <p className="rt-field-hint">
            {t(
              contextCustom
                ? "intentRouting.contextCustomHelp"
                : group.contextWindow === -1
                  ? "intentRouting.contextSmallestHelp"
                  : "intentRouting.contextHelp",
              {
                tokens: Number.isFinite(group.contextWindow)
                  ? group.contextWindow?.toLocaleString()
                  : "—",
              },
            )}
          </p>
        </div>
      </div>
      <div className="rt-setting-row">
        <span>{t("routingPage.stays")}</span>
        <div className="rt-setting-content">
          <RoutingSegments
            label={t("intentRouting.affinity")}
            value={group.affinity || "session"}
            disabled={disabled}
            options={(["session", "turn", "off", "strict"] as const).map(
              (value) => ({
                value,
                label: t(`intentRouting.affinityOptions.${value}`),
              }),
            )}
            onChange={(affinity) => onChange({ ...group, affinity })}
          />
          <p className="rt-field-hint">{t("intentRouting.affinityHelp")}</p>
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
