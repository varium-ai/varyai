import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronDown,
  GripVertical,
  Image,
  List,
  Network,
  Search,
} from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import "./model-picker.css";

export interface ModelChoice {
  id: string;
  name: string;
  group?: string;
  modelId?: string;
  icon?: string | null;
  contextWindow?: number | null;
  reasoningLevels?: string[];
  images?: boolean | null;
  disabled?: boolean;
}

export interface ModelVisibilityControls {
  only: boolean;
  order?: string[];
  onOnlyChange: (only: boolean) => void;
  onShownChange: (ids: string[], visible: boolean) => void;
  onOrderChange: (ids: string[]) => void;
}

const modelName = (model: ModelChoice) => {
  const suffix = ` · ${model.group === "routing" ? "routing group" : model.group}`;
  return model.name.endsWith(suffix)
    ? model.name.slice(0, -suffix.length)
    : model.name;
};

function ModelCapabilities({ model }: { model: ModelChoice }) {
  const { t, i18n } = useTranslation();
  const format = (value: number, compact = false) =>
    Intl.NumberFormat(
      i18n.resolvedLanguage ?? i18n.language ?? "en",
      compact ? { notation: "compact", maximumFractionDigits: 1 } : {},
    ).format(value);
  return (
    <span
      className="mp-model-info"
      title={[
        model.reasoningLevels?.join(", "),
        model.images
          ? t("modelProviders.acceptsImages")
          : model.images === false
            ? t("modelProviders.textOnly")
            : "",
        model.contextWindow ? format(model.contextWindow) : "",
      ]
        .filter(Boolean)
        .join("\n")}
    >
      {!!model.reasoningLevels?.length && (
        <span className="mp-model-badge">
          {model.reasoningLevels.length > 2
            ? `${model.reasoningLevels[0]}–${model.reasoningLevels[model.reasoningLevels.length - 1]}`
            : model.reasoningLevels.join(" / ")}
        </span>
      )}
      {model.images && (
        <Image
          size={11}
          role="img"
          aria-label={t("modelProviders.acceptsImages")}
        />
      )}
      {model.contextWindow != null && (
        <span className="mp-am-context">
          {format(model.contextWindow, true)}
        </span>
      )}
    </span>
  );
}

export function SharedModelPicker({
  models,
  selected,
  onPick,
  multiple = false,
  disabled = false,
  visibility,
  title,
  description,
  descriptionTitle,
  name,
  error,
  saving,
}: {
  models: ModelChoice[];
  selected: string[];
  onPick: (id: string) => void;
  multiple?: boolean;
  disabled?: boolean;
  visibility?: ModelVisibilityControls;
  title?: string;
  description?: string;
  descriptionTitle?: string;
  name?: string;
  error?: string | null;
  saving?: boolean;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [view, setView] = useState<"all" | "shown" | "order">("all");
  const [provider, setProvider] = useState<string | null>(null);
  const [kept, setKept] = useState<Set<string>>(new Set());
  const [armed, setArmed] = useState<"hide" | "show" | null>(null);
  const [wide, setWide] = useState(() => window.innerWidth >= 600);
  const list = useRef<HTMLDivElement>(null);
  const groups = new Map<string, ModelChoice[]>();
  for (const model of models) {
    const group = model.group ?? "";
    groups.set(group, [...(groups.get(group) ?? []), model]);
  }
  const sections = [...groups].sort(
    ([a], [b]) => +(b === "routing") - +(a === "routing"),
  );
  const [folded, setFolded] = useState(() => {
    const providers = sections.filter(([group]) => group !== "routing").length;
    return new Set(
      sections
        .filter(
          ([group, rows]) =>
            providers > 1 &&
            group !== "routing" &&
            rows.length > 8 &&
            (!!visibility || !rows.some((m) => selected.includes(m.id))),
        )
        .map(([group]) => group),
    );
  });
  const railed = groups.size > 2 && wide;
  useEffect(() => {
    const resize = () => setWide(window.innerWidth >= 600);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  useEffect(() => {
    if (!armed) return;
    const timer = window.setTimeout(() => setArmed(null), 4000);
    return () => window.clearTimeout(timer);
  }, [armed]);
  const groupName = (group: string) =>
    group === "routing"
      ? t("workspaceNav.routing")
      : group || t("modelProviders.title");
  const groupIcon = (group: string) =>
    group === "routing" ? (
      <Network size={14} aria-hidden />
    ) : (
      <ProviderIcon
        icon={groups.get(group)?.[0]?.icon ?? "generic"}
        name={groupName(group)}
        size={14}
      />
    );
  function setShown(rows: ModelChoice[], visible: boolean) {
    if (!visibility || disabled) return;
    if (!visible) setKept((old) => new Set([...old, ...rows.map((m) => m.id)]));
    setArmed(null);
    visibility.onShownChange(
      rows.map((m) => m.id),
      visible,
    );
  }
  function switchView(next: typeof view) {
    setView(next);
    setKept(new Set());
    setArmed(null);
    list.current?.scrollTo?.({ top: 0 });
  }
  function move(from: string, to: string) {
    if (!visibility || disabled || from === to) return;
    const ids = inOrder.map((m) => m.id);
    const source = ids.indexOf(from),
      target = ids.indexOf(to);
    if (source < 0 || target < 0) return;
    ids.splice(target, 0, ...ids.splice(source, 1));
    visibility.onOrderChange(ids);
  }
  const drag = useRoutingRowDrag("data-agent-model", move);
  const positions = new Map(visibility?.order?.map((id, index) => [id, index]));
  const inOrder = models
    .filter((m) => selected.includes(m.id))
    .sort(
      (a, b) =>
        (positions.get(a.id) ?? Infinity) - (positions.get(b.id) ?? Infinity),
    );
  const term = query.trim().toLocaleLowerCase();
  const matches = (model: ModelChoice) =>
    (!term ||
      `${model.name} ${model.modelId ?? model.id} ${groupName(model.group ?? "")}`
        .toLocaleLowerCase()
        .includes(term)) &&
    (view !== "shown" || selected.includes(model.id) || kept.has(model.id));
  const visibleSections = sections
    .filter(([group]) => provider === null || !!term || group === provider)
    .map(([group, rows]) => [group, rows.filter(matches)] as const)
    .filter(([, rows]) => rows.length);
  function bulk(action: "hide" | "show") {
    if (armed !== action) {
      setArmed(action);
      return;
    }
    setShown(models, action === "show");
  }
  return (
    <div
      className={`mp-shared-picker${railed ? " railed" : ""}`}
      onKeyDown={(event) => {
        if (view === "order") return;
        const rows = Array.from(
          list.current?.querySelectorAll<HTMLButtonElement>(
            "[data-model-choice]:not(:disabled)",
          ) ?? [],
        );
        const at = rows.indexOf(event.target as HTMLButtonElement);
        if (["ArrowDown", "ArrowUp"].includes(event.key)) {
          if (!rows.length) return;
          event.preventDefault();
          const next =
            at < 0
              ? event.key === "ArrowDown"
                ? 0
                : rows.length - 1
              : (at + (event.key === "ArrowDown" ? 1 : -1) + rows.length) %
                rows.length;
          rows[next]?.focus({ preventScroll: true });
          rows[next]?.scrollIntoView?.({ block: "nearest" });
        } else if (event.key === "Enter" && at >= 0) {
          event.preventDefault();
          rows[at].click();
        }
      }}
    >
      {(title || description) && (
        <header className="mp-am-head">
          {title && <h3>{title}</h3>}
          {description && <p title={descriptionTitle}>{description}</p>}
        </header>
      )}
      <div className="mp-am-tools" hidden={view === "order"}>
        <label className="mp-am-search">
          <Search size={12} aria-hidden />
          <input
            type="search"
            autoComplete="off"
            spellCheck={false}
            aria-label={t("modelPicker.search")}
            placeholder={t("agentConnections.searchModels")}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setArmed(null);
              list.current?.scrollTo?.({ top: 0 });
            }}
          />
        </label>
        {visibility && (
          <div
            className="mp-am-seg"
            role="group"
            aria-label={t("agentConnections.modelListView")}
          >
            {(["all", "shown", "order"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                aria-pressed={view === mode}
                onClick={() => switchView(mode)}
              >
                {t(
                  `agentConnections.modelView${{ all: "All", shown: "Shown", order: "Order" }[mode]}`,
                )}
              </button>
            ))}
          </div>
        )}
      </div>
      {view === "order" && (
        <div className="mp-am-order-tools">
          <div
            className="mp-am-seg"
            role="group"
            aria-label={t("agentConnections.modelListView")}
          >
            {(["all", "shown", "order"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                aria-pressed={view === mode}
                onClick={() => switchView(mode)}
              >
                {t(
                  `agentConnections.modelView${{ all: "All", shown: "Shown", order: "Order" }[mode]}`,
                )}
              </button>
            ))}
          </div>
        </div>
      )}
      <div className="mp-am-body">
        {railed && view !== "order" && (
          <nav
            className="mp-am-rail"
            aria-label={t("agentConnections.modelProviders")}
          >
            {(
              [[null, models], ...sections] as [string | null, ModelChoice[]][]
            ).map(([id, entries]) => {
              return (
                <button
                  type="button"
                  key={id === null ? "all" : `provider/${id}`}
                  aria-pressed={provider === id}
                  aria-label={
                    id !== null
                      ? groupName(id)
                      : t("agentConnections.allProviders")
                  }
                  onClick={() => {
                    setProvider(id);
                    setQuery("");
                    setArmed(null);
                    list.current?.scrollTo?.({ top: 0 });
                  }}
                >
                  {id !== null ? groupIcon(id) : <List size={14} aria-hidden />}
                  <span>
                    {id !== null
                      ? groupName(id)
                      : t("agentConnections.allProviders")}
                  </span>
                  <small>
                    {entries.filter((m) => selected.includes(m.id)).length}/
                    {entries.length}
                  </small>
                </button>
              );
            })}
          </nav>
        )}
        <div
          className="mp-am-list"
          ref={list}
          role={!multiple && !visibility ? "radiogroup" : undefined}
          aria-label={t("modelPicker.models")}
        >
          {view === "order" ? (
            <div role="list" data-routing-list className="mp-am-order">
              {inOrder.map((model, index) => (
                <div
                  role="listitem"
                  tabIndex={0}
                  aria-label={modelName(model)}
                  key={model.id}
                  data-agent-model={model.id}
                  className="mp-am-row mp-am-order-row"
                  onKeyDown={(event) => {
                    if (
                      disabled ||
                      !event.altKey ||
                      !["ArrowUp", "ArrowDown"].includes(event.key)
                    )
                      return;
                    event.preventDefault();
                    const target =
                      inOrder[index + (event.key === "ArrowUp" ? -1 : 1)];
                    if (target) move(model.id, target.id);
                  }}
                >
                  <button
                    className="mp-am-grip"
                    type="button"
                    disabled={disabled}
                    aria-label={`${t("agentConnections.dragModel")} ${modelName(model)}`}
                    onPointerDown={(event) => drag.start(event, model.id)}
                    onClick={() => drag.consumeClick(model.id)}
                  >
                    <GripVertical size={12} />
                  </button>
                  <small>{index + 1}</small>
                  <span className="mp-am-name">{modelName(model)}</span>
                  <span className="mp-am-context">
                    {groupName(model.group ?? "")}
                  </span>
                </div>
              ))}
              {!inOrder.length && (
                <p className="mp-am-empty">{t("modelPicker.empty")}</p>
              )}
            </div>
          ) : (
            <>
              {visibleSections.map(([group, rows]) => {
                const all = groups.get(group)!;
                const isFolded =
                  !term && provider === null && folded.has(group);
                const count = all.filter((m) => selected.includes(m.id)).length;
                return (
                  <section key={group} className="mp-am-group">
                    <div className="mp-am-group-head">
                      <button
                        type="button"
                        className="mp-am-fold"
                        aria-expanded={!isFolded}
                        aria-label={groupName(group)}
                        onClick={() => {
                          setFolded((old) => {
                            const next = new Set(old);
                            next.has(group)
                              ? next.delete(group)
                              : next.add(group);
                            return next;
                          });
                          setArmed(null);
                        }}
                      >
                        <ChevronDown
                          size={10}
                          className={isFolded ? "folded" : ""}
                          aria-hidden
                        />
                        {groupIcon(group)}
                        <span>{groupName(group)}</span>
                        <small>
                          {count} / {all.length}
                        </small>
                      </button>
                      {visibility && (
                        <button
                          className="mp-am-group-all"
                          type="button"
                          aria-label={`${t(count === all.length ? "agentConnections.hideAll" : "agentConnections.showAll")} ${groupName(group)}`}
                          disabled={disabled}
                          onClick={() => setShown(all, count !== all.length)}
                        >
                          {t(
                            count === all.length
                              ? "agentConnections.hideAll"
                              : "agentConnections.showAll",
                          )}
                        </button>
                      )}
                    </div>
                    {!isFolded && (
                      <div>
                        {rows.map((model) => {
                          const visible = selected.includes(model.id);
                          return (
                            <button
                              type="button"
                              role={
                                visibility
                                  ? "menuitemcheckbox"
                                  : multiple
                                    ? "checkbox"
                                    : "radio"
                              }
                              aria-checked={visible}
                              aria-label={modelName(model)}
                              key={model.id}
                              title={model.modelId ?? model.id}
                              data-model-choice
                              className={`mp-am-row${visibility && !visible ? " off" : ""}`}
                              disabled={disabled || model.disabled}
                              onClick={() => {
                                if (!multiple && !visibility && visible) return;
                                if (visibility && visible)
                                  setKept((old) => new Set([...old, model.id]));
                                setArmed(null);
                                onPick(model.id);
                              }}
                            >
                              <span className="mp-am-logo" aria-hidden>
                                {groupIcon(group)}
                              </span>
                              <span className="mp-am-name">
                                {modelName(model)}
                              </span>
                              <ModelCapabilities model={model} />
                              <span className="mp-am-check" aria-hidden>
                                {visible && (
                                  <Check size={12} strokeWidth={1.9} />
                                )}
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </section>
                );
              })}
              {!visibleSections.length && (
                <p className="mp-am-empty">{t("modelPicker.empty")}</p>
              )}
            </>
          )}
        </div>
      </div>
      {error && (
        <p role="alert" className="mp-am-error">
          {t(error)}
        </p>
      )}
      {saving && (
        <span role="status" className="sr-only">
          {t("modelPicker.saving")}
        </span>
      )}
      {visibility && (
        <footer className="mp-am-foot">
          {view === "order" ? (
            <>
              <span className="mp-am-order-hint">
                {t("agentConnections.modelOrderHint", { agent: name })}
              </span>
              <button
                type="button"
                disabled={disabled || !visibility.order?.length}
                onClick={() => visibility.onOrderChange([])}
              >
                {t("agentConnections.defaultModelOrder")}
              </button>
            </>
          ) : (
            <>
              {!armed && (
                <label
                  className="mp-am-only"
                  title={t(
                    visibility.only
                      ? "agentConnections.newModelsHidden"
                      : "agentConnections.newModelsShown",
                  )}
                >
                  <button
                    type="button"
                    role="switch"
                    aria-checked={visibility.only ?? false}
                    aria-label={t("agentConnections.onlyPicked")}
                    disabled={disabled}
                    onClick={() => visibility.onOnlyChange(!visibility.only)}
                  >
                    <i />
                  </button>
                  <span>{t("agentConnections.onlyPicked")}</span>
                </label>
              )}
              <span className="mp-am-spacer" />
              <button
                type="button"
                className={armed === "hide" ? "armed" : ""}
                disabled={disabled || !selected.length}
                onClick={() => bulk("hide")}
              >
                {t(
                  armed === "hide"
                    ? "agentConnections.confirmHideAll"
                    : "agentConnections.hideAll",
                  { count: selected.length },
                )}
              </button>
              <span aria-hidden>·</span>
              <button
                type="button"
                className={armed === "show" ? "armed" : ""}
                disabled={disabled || selected.length === models.length}
                onClick={() => bulk("show")}
              >
                {t(
                  armed === "show"
                    ? "agentConnections.confirmShowAll"
                    : "agentConnections.showAll",
                  { count: models.length - selected.length },
                )}
              </button>
            </>
          )}
        </footer>
      )}

      {multiple && !visibility && (
        <p className="mp-am-foot">
          {t("modelPicker.multiHint", { count: selected.length })}
        </p>
      )}
    </div>
  );
}
