import { useLayoutEffect, useRef, useState } from "react";
import { Info, MoreHorizontal, Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  RoutingRuleConditions,
  pauseRoutingRule,
} from "./RoutingRuleConditions";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import type { IntentRule, RoutingMember } from "@/lib/api/intentRouting";

export function RoutingRulesEditor({
  rules,
  members,
  disabled,
  onChange,
}: {
  rules: IntentRule[];
  members: RoutingMember[];
  disabled: boolean;
  onChange: (rules: IntentRule[]) => void;
}) {
  const { t } = useTranslation();
  const list = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<string | null>(null);
  const [reasoning, setReasoning] = useState(false);
  const [focusRule, setFocusRule] = useState<string | null>(null);
  useLayoutEffect(() => {
    if (!focusRule) return;
    const frame = requestAnimationFrame(() => {
      Array.from(
        list.current?.querySelectorAll<HTMLElement>("[data-routing-rule]") ??
          [],
      )
        .find((row) => row.getAttribute("data-routing-rule") === focusRule)
        ?.querySelector<HTMLTextAreaElement>("textarea")
        ?.focus({ preventScroll: false });
      setFocusRule(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focusRule]);
  useLayoutEffect(() => {
    const host = list.current;
    if (!host || typeof ResizeObserver === "undefined") return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        host.style.removeProperty("--rt-condw");
        const width = Math.max(
          0,
          ...Array.from(
            host.querySelectorAll<HTMLElement>(".rt-rule > .when"),
          ).map((line) => line.getBoundingClientRect().width),
        );
        if (width)
          host.style.setProperty(
            "--rt-condw",
            `${Math.ceil(width * 64) / 64}px`,
          );
      });
    };
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    measure();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [rules]);
  function update(rule: IntentRule) {
    onChange(rules.map((old) => (old.id === rule.id ? rule : old)));
  }
  function move(from: number, to: number, keep?: "handle" | "menu") {
    if (disabled || to < 0 || to >= rules.length || from === to) return;
    const next = [...rules];
    next.splice(to, 0, ...next.splice(from, 1));
    onChange(next);
    if (keep)
      requestAnimationFrame(() =>
        list.current
          ?.querySelectorAll<HTMLElement>(
            keep === "handle" ? ".rt-rhandle" : ".rt-rmore",
          )
          [to]?.focus({ preventScroll: true }),
      );
  }
  const drag = useRoutingRowDrag("data-routing-rule", (from, to) =>
    move(
      rules.findIndex((rule) => rule.id === from),
      rules.findIndex((rule) => rule.id === to),
    ),
  );
  const name = (id: string) => {
    const member = members.find((member) => member.id === id);
    return member
      ? `${member.modelId}${member.effort ? ` :${member.effort}` : ""}`
      : id;
  };
  const memberMenu = (ids: string[], select: (id: string) => void) =>
    ids.map((id) => (
      <DropdownMenuItem key={id} onSelect={() => select(id)}>
        {name(id)}
      </DropdownMenuItem>
    ));
  return (
    <div className="rt-rules">
      <div className="fbl" data-routing-list ref={list}>
        {rules.map((rule, index) => {
          const uses = [
            rule.targetMemberId,
            ...(rule.pause ? [] : (rule.fallbackMemberIds ?? [])),
          ].filter(Boolean);
          const setUses = (ids: string[]) =>
            update({
              ...rule,
              targetMemberId: ids[0],
              fallbackMemberIds: ids.slice(1),
            });
          const left = members
            .filter((member) => !uses.includes(member.id))
            .map((member) => member.id);
          return (
            <div
              key={rule.id}
              className={`rt-rule${rule.pause ? " pause" : ""}${rule.enabled ? "" : " rt-rule-off"}`}
              data-routing-rule={rule.id}
              aria-label={t("intentRouting.ruleNumber", { n: index + 1 })}
            >
              <button
                className="rt-rhandle"
                type="button"
                disabled={disabled}
                aria-label={t("routingPage.moveRule", { n: index + 1 })}
                title={t("routingPage.dragHint")}
                onPointerDown={(event) => {
                  if (!disabled) drag.start(event, rule.id);
                }}
                onClick={() => drag.consumeClick(rule.id)}
                onKeyDown={(event) => {
                  if (
                    event.altKey &&
                    ["ArrowUp", "ArrowDown"].includes(event.key)
                  ) {
                    event.preventDefault();
                    event.stopPropagation();
                    move(
                      index,
                      index + (event.key === "ArrowUp" ? -1 : 1),
                      "handle",
                    );
                  }
                }}
              >
                {index + 1}
              </button>
              <RoutingRuleConditions
                rule={rule}
                disabled={disabled}
                onChange={update}
              />
              <div className="rt-line rt-then">
                <span className="rt-arrow" aria-hidden="true">
                  &rarr;
                </span>
                {rule.pause && (
                  <span className="rt-cond on">
                    {t("intentRouting.pauseMember")}
                  </span>
                )}
                <div className="rt-line rt-uses">
                  {uses.map((id, at) => (
                    <span className="rt-c" key={id}>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            className="rt-cond on rt-model"
                            disabled={disabled}
                            aria-label={`${t("intentRouting.target")} ${name(id)}`}
                          >
                            <span>{name(id)}</span>
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent className="mp-row-menu">
                          {memberMenu([id, ...left], (next) =>
                            setUses(
                              uses.map((old, i) => (i === at ? next : old)),
                            ),
                          )}
                        </DropdownMenuContent>
                      </DropdownMenu>
                      {uses.length > 1 && (
                        <button
                          className="rt-cx"
                          type="button"
                          disabled={disabled}
                          aria-label={`${t("routingPage.removeRuleModel")} ${name(id)}`}
                          onClick={() =>
                            setUses(uses.filter((_, i) => i !== at))
                          }
                        >
                          <X size={8} />
                        </button>
                      )}
                    </span>
                  ))}
                  {!rule.pause && !!left.length && (
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <button
                          type="button"
                          className="rt-adduse"
                          disabled={disabled}
                          aria-label={t("routingPage.addRuleModel")}
                        >
                          <Plus size={11} />
                        </button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent className="mp-row-menu">
                        {memberMenu(left, (id) => setUses([...uses, id]))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  )}
                </div>
                {!rule.pause && rule.useEffort && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        disabled={disabled}
                        className="rt-cond on"
                        aria-label={t("intentRouting.ruleUseEffort")}
                      >
                        {rule.useEffort}
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent className="mp-row-menu">
                      {["", "low", "medium", "high", "xhigh", "max"].map(
                        (level) => (
                          <DropdownMenuItem
                            key={level}
                            onSelect={() =>
                              update({ ...rule, useEffort: level })
                            }
                          >
                            {level || t("intentRouting.memberEffortAgent")}
                          </DropdownMenuItem>
                        ),
                      )}
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>
              <DropdownMenu
                open={menu === rule.id}
                onOpenChange={(open) => {
                  setMenu(open ? rule.id : null);
                  setReasoning(false);
                }}
              >
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    className="rt-rmore mp-icon-button"
                    disabled={disabled}
                    aria-label={t("routingPage.ruleActions", { n: index + 1 })}
                  >
                    <MoreHorizontal size={14} />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent className="mp-row-menu" align="end">
                  {reasoning ? (
                    ["", "low", "medium", "high", "xhigh", "max"].map(
                      (level) => (
                        <DropdownMenuItem
                          key={level}
                          onSelect={() => {
                            update({ ...rule, useEffort: level });
                            setMenu(null);
                          }}
                        >
                          {level || t("intentRouting.memberEffortAgent")}
                        </DropdownMenuItem>
                      ),
                    )
                  ) : (
                    <>
                      <DropdownMenuItem
                        disabled={index === 0}
                        onSelect={() => move(index, index - 1, "menu")}
                      >
                        {t("intentRouting.moveUp")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={index === rules.length - 1}
                        onSelect={() => move(index, index + 1, "menu")}
                      >
                        {t("intentRouting.moveDown")}
                      </DropdownMenuItem>
                      {!rule.pause && (
                        <>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onSelect={(event) => {
                              event.preventDefault();
                              setReasoning(true);
                            }}
                          >
                            {t("intentRouting.ruleUseEffort")}
                          </DropdownMenuItem>
                        </>
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onSelect={() =>
                          update(pauseRoutingRule(rule, !rule.pause))
                        }
                      >
                        {t(
                          rule.pause
                            ? "routingPage.sendRule"
                            : "routingPage.pauseRule",
                        )}
                      </DropdownMenuItem>
                      {!rule.enabled && (
                        <DropdownMenuItem
                          onSelect={() => update({ ...rule, enabled: true })}
                        >
                          {t("intentRouting.ruleEnabled")}
                        </DropdownMenuItem>
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onSelect={() =>
                          onChange(rules.filter((old) => old.id !== rule.id))
                        }
                      >
                        {t("routingPage.removeRule")}
                      </DropdownMenuItem>
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          );
        })}
      </div>
      <div className="rt-gadds">
        <button
          type="button"
          className="rt-gadd"
          disabled={disabled || members.length < 2 || rules.length >= 16}
          onClick={() => {
            const id = crypto.randomUUID();
            onChange([
              ...rules,
              {
                id,
                name: t("intentRouting.ruleNumber", { n: rules.length + 1 }),
                description: "",
                enabled: true,
                targetMemberId: members[members.length - 1]?.id ?? "",
              },
            ]);
            setFocusRule(id);
          }}
        >
          <Plus size={11} />
          {t("intentRouting.addRule")}
        </button>
        <span
          tabIndex={0}
          role="img"
          className="rt-info"
          aria-label={t("intentRouting.conditionsHelp")}
          title={t("intentRouting.conditionsHelp")}
        >
          <Info size={14} />
        </span>
      </div>
      {members.length < 2 && (
        <p className="rt-rule-hint">{t("routingPage.ruleNeedsModels")}</p>
      )}
    </div>
  );
}
