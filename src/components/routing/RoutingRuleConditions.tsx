import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { APP_IDS } from "@/config/appConfig";
import type { IntentRule, RoutingConditions } from "@/lib/api/intentRouting";

const kinds = [
  "intent",
  "tokens",
  "images",
  "effort",
  "compact",
  "agents",
  "folders",
  "exceptFolders",
  "time",
] as const;
type Kind = (typeof kinds)[number];
const pauseKinds: readonly Kind[] = [
  "agents",
  "folders",
  "exceptFolders",
  "time",
];
const agents = [...APP_IDS.filter((app) => app !== "cursor-cli"), "external"];

export function pauseRoutingRule(rule: IntentRule, pause: boolean): IntentRule {
  if (!pause) return { ...rule, pause: false };
  const { time, agents, folders, exceptFolders } = rule.conditions ?? {};
  const active =
    time || agents?.length || folders?.length || exceptFolders?.length;
  return {
    ...rule,
    pause: true,
    description: "",
    useEffort: "",
    fallbackMemberIds: [],
    conditions: active ? { time, agents, folders, exceptFolders } : undefined,
  };
}

export function RoutingRuleConditions({
  rule,
  disabled,
  onChange,
}: {
  rule: IntentRule;
  disabled: boolean;
  onChange: (rule: IntentRule) => void;
}) {
  const { t } = useTranslation();
  const host = useRef<HTMLDivElement>(null);
  const [opened, setOpened] = useState<Kind[]>(
    rule.description ? [] : !rule.conditions && !rule.pause ? ["intent"] : [],
  );
  const [focus, setFocus] = useState<Kind | "+" | null>(null);
  useEffect(() => {
    if (rule.pause)
      setOpened((old) => old.filter((kind) => pauseKinds.includes(kind)));
  }, [rule.pause]);
  const has = (kind: Kind) =>
    kind === "intent"
      ? !!rule.description
      : Array.isArray(rule.conditions?.[kind])
        ? !!(rule.conditions?.[kind] as string[]).length
        : !!rule.conditions?.[kind];
  const allowed = kinds.filter(
    (kind) => !rule.pause || pauseKinds.includes(kind),
  );
  const shown = allowed.filter((kind) => has(kind) || opened.includes(kind));
  const free = allowed.filter((kind) => !shown.includes(kind));
  const label = (kind: Kind) =>
    t(`intentRouting.condition${kind[0].toUpperCase()}${kind.slice(1)}`);
  useLayoutEffect(() => {
    if (!focus) return;
    const selector =
      focus === "+"
        ? ".rt-addc"
        : `[data-condition="${focus}"] :is(input, textarea, button, select)`;
    const frame = requestAnimationFrame(() => {
      host.current
        ?.querySelector<HTMLElement>(selector)
        ?.focus({ preventScroll: true });
      setFocus(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focus, opened]);
  function update(patch: Partial<RoutingConditions>, retain = true) {
    if (retain) setOpened((old) => [...new Set([...old, ...shown])]);
    const conditions = { ...rule.conditions, ...patch };
    const active = Object.values(conditions).some((value) =>
      Array.isArray(value) ? value.length > 0 : !!value,
    );
    onChange({ ...rule, conditions: active ? conditions : undefined });
  }
  function remove(kind: Kind) {
    setOpened(opened.filter((value) => value !== kind));
    if (kind === "intent") onChange({ ...rule, description: "" });
    else update({ [kind]: undefined }, false);
    setFocus("+");
  }
  function add(kind: Kind) {
    setOpened([...opened, kind]);
    if (kind === "images" || kind === "compact") update({ [kind]: true });
    if (kind === "effort") update({ effort: "on" });
    setFocus(kind);
  }
  const time = rule.conditions?.time ?? {
    from: "",
    to: "",
    zone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    days: [],
  };
  const timePatch = (patch: Partial<typeof time>) =>
    update({ time: { ...time, ...patch } });
  return (
    <div
      className="rt-line when"
      ref={host}
      onKeyDown={(event) => {
        if (
          event.key === "Enter" &&
          (event.target instanceof HTMLInputElement ||
            event.target instanceof HTMLTextAreaElement)
        ) {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
    >
      {shown.map((kind) => (
        <span className="rt-c" data-condition={kind} key={kind}>
          {kind === "intent" ? (
            <label
              className={`rt-cond in${rule.description.trim() ? " on" : ""}`}
            >
              <span>{t("routingPage.asksFor")}</span>
              <span
                className="rt-grow"
                data-v={rule.description || t("routingPage.intentPlaceholder")}
              >
                <textarea
                  aria-label={t("intentRouting.description")}
                  rows={1}
                  cols={1}
                  maxLength={200}
                  disabled={disabled}
                  spellCheck={false}
                  autoComplete="off"
                  placeholder={t("routingPage.intentPlaceholder")}
                  value={rule.description}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.preventDefault();
                    event.stopPropagation();
                  }}
                  onChange={(event) => {
                    setOpened((old) =>
                      old.includes(kind) ? old : [...old, kind],
                    );
                    onChange({
                      ...rule,
                      description: event.target.value.replace(/[\r\n]+/g, " "),
                    });
                  }}
                />
              </span>
            </label>
          ) : kind === "tokens" ? (
            <label
              className={`rt-cond tk${rule.conditions?.tokens ? " on" : ""}`}
            >
              <span aria-hidden="true">&ge;</span>
              <input
                aria-label={label(kind)}
                type="number"
                min={0}
                max={100000000}
                step={1000}
                disabled={disabled}
                value={rule.conditions?.tokens || ""}
                onChange={(event) =>
                  update({ tokens: Math.max(0, Number(event.target.value)) })
                }
              />
              <span>{t("routingPage.ruleTokens")}</span>
            </label>
          ) : kind === "folders" || kind === "exceptFolders" ? (
            <label
              className={`rt-cond fd${rule.conditions?.[kind]?.length ? " on" : ""}`}
            >
              <span>{label(kind)}</span>
              <input
                aria-label={label(kind)}
                disabled={disabled}
                maxLength={32768}
                value={rule.conditions?.[kind]?.join(", ") ?? ""}
                placeholder="~/work, ~/oss"
                onChange={(event) =>
                  update({
                    [kind]: event.target.value
                      .split(",")
                      .map((value) => value.trim())
                      .filter(Boolean),
                  })
                }
              />
            </label>
          ) : kind === "images" || kind === "compact" ? (
            <button
              type="button"
              className={`rt-cond${rule.conditions?.[kind] ? " on" : ""}`}
              disabled={disabled}
              aria-pressed={!!rule.conditions?.[kind]}
              onClick={() => update({ [kind]: !rule.conditions?.[kind] })}
            >
              {label(kind)}
            </button>
          ) : kind === "effort" ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className={`rt-cond${rule.conditions?.effort ? " on" : ""}`}
                  disabled={disabled}
                  aria-label={label(kind)}
                >
                  {rule.conditions?.effort === "on"
                    ? t("intentRouting.anyReasoning")
                    : `${label(kind)} ${rule.conditions?.effort ?? ""}`}
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="mp-row-menu">
                {["", "on", "low", "medium", "high", "xhigh", "max"].map(
                  (value) => (
                    <DropdownMenuItem
                      key={value}
                      onSelect={() => update({ effort: value })}
                    >
                      {value === ""
                        ? t("intentRouting.memberEffortAgent")
                        : value === "on"
                          ? t("intentRouting.anyReasoning")
                          : value}
                    </DropdownMenuItem>
                  ),
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : kind === "agents" ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  disabled={disabled}
                  className={`rt-cond${rule.conditions?.agents?.length ? " on" : ""}`}
                  aria-label={label(kind)}
                >
                  {rule.conditions?.agents?.join(", ") || label(kind)}
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="mp-row-menu">
                {agents.map((agent) => (
                  <DropdownMenuCheckboxItem
                    key={agent}
                    checked={rule.conditions?.agents?.includes(agent) ?? false}
                    onSelect={(event) => event.preventDefault()}
                    onCheckedChange={(checked) =>
                      update({
                        agents: checked
                          ? [...(rule.conditions?.agents ?? []), agent]
                          : rule.conditions?.agents?.filter(
                              (value) => value !== agent,
                            ),
                      })
                    }
                  >
                    {agent}
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  disabled={disabled}
                  className={`rt-cond${rule.conditions?.time ? " on" : ""}`}
                  aria-label={label(kind)}
                >
                  {time.from && time.to
                    ? `${time.from} - ${time.to}`
                    : label(kind)}
                  {time.days?.length
                    ? ` · ${time.days.map((day) => t(`intentRouting.weekdays.${day}`)).join(", ")}`
                    : ""}
                </button>
              </PopoverTrigger>
              <PopoverContent className="mp-rule-time" align="start">
                <div className="rt-rule-time-fields">
                  {(["from", "to", "zone"] as const).map((key) => (
                    <label key={key}>
                      {t(
                        `intentRouting.${key === "from" ? "timeFrom" : key === "to" ? "timeTo" : "timeZone"}`,
                      )}
                      <input
                        type={key === "zone" ? "text" : "time"}
                        aria-label={t(
                          `intentRouting.${key === "from" ? "timeFrom" : key === "to" ? "timeTo" : "timeZone"}`,
                        )}
                        disabled={disabled}
                        maxLength={100}
                        value={time[key]}
                        onChange={(event) =>
                          timePatch({ [key]: event.target.value })
                        }
                      />
                    </label>
                  ))}
                  <fieldset>
                    <legend>{t("intentRouting.timeDays")}</legend>
                    {["mon", "tue", "wed", "thu", "fri", "sat", "sun"].map(
                      (day) => (
                        <label key={day}>
                          <input
                            type="checkbox"
                            disabled={disabled}
                            checked={time.days?.includes(day) ?? false}
                            onChange={(event) =>
                              timePatch({
                                days: event.target.checked
                                  ? [...(time.days ?? []), day]
                                  : time.days?.filter((value) => value !== day),
                              })
                            }
                          />
                          {t(`intentRouting.weekdays.${day}`)}
                        </label>
                      ),
                    )}
                  </fieldset>
                </div>
              </PopoverContent>
            </Popover>
          )}
          <button
            type="button"
            className="rt-cx"
            disabled={disabled}
            aria-label={`${t("intentRouting.removeCondition")} ${label(kind)}`}
            onClick={() => remove(kind)}
          >
            <X size={8} />
          </button>
        </span>
      ))}
      {!!free.length && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              disabled={disabled}
              className={`rt-addc${shown.length ? "" : " wide"}`}
              aria-label={t("intentRouting.addCondition")}
            >
              <Plus size={11} />
              {!shown.length && t("intentRouting.addCondition")}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="mp-row-menu"
            onCloseAutoFocus={(event) => {
              if (focus) event.preventDefault();
            }}
          >
            {free.map((kind) => (
              <DropdownMenuItem key={kind} onSelect={() => add(kind)}>
                {label(kind)}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}
