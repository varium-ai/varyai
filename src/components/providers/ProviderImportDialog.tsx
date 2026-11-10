import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Box, FolderOpen, Loader2 } from "lucide-react";
import { open as chooseFile } from "@tauri-apps/plugin-dialog";
import {
  providerImportApi,
  type ProviderImportItem,
  type ProviderImportPick,
  type ProviderImportSource,
} from "@/lib/api/providerImport";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ProviderImportSourceIcon } from "./ProviderImportSourceIcon";
import "./providers.css";

type Choice = { on: boolean; mode: "key" | "add"; name?: string };
const pickable = (item: ProviderImportItem) =>
  !item.skip && item.status !== "same";
const keyOf = (source: string, reference: string) =>
  JSON.stringify([source, reference]);

export function ProviderImportDialog({
  open,
  onClose,
  onImported,
  onBusyChange,
  embedded = false,
}: {
  open: boolean;
  onClose: () => void;
  onImported: () => void | Promise<void>;
  onBusyChange?: (busy: boolean) => void;
  embedded?: boolean;
}) {
  const { t } = useTranslation();
  const [sources, setSources] = useState<ProviderImportSource[]>([]);
  const [tab, setTab] = useState("");
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sourcePath, setSourcePath] = useState<string>();
  const saving = useRef(false);
  const returnFocus = useRef<HTMLElement | null>(null);
  const allBox = useRef<HTMLInputElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  useEffect(() => {
    onBusyChange?.(busy);
    return () => onBusyChange?.(false);
  }, [busy, onBusyChange]);
  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setError("");
    setSources([]);
    setChoices({});
    const request = sourcePath
      ? providerImportApi.preview(sourcePath)
      : providerImportApi.preview();
    void request
      .then((next) => {
        if (!active) return;
        const ordered = [...next].sort(
          (a, b) => Number(b.id === "magpie") - Number(a.id === "magpie"),
        );
        const initial: Record<string, Choice> = {};
        for (const source of next)
          for (const item of source.items) {
            if (pickable(item))
              initial[keyOf(source.id, item.reference)] = {
                on: item.enabled && item.status !== "taken",
                mode: item.status === "vendor" ? "key" : "add",
              };
          }
        setSources(ordered);
        setChoices(initial);
        setTab(
          (embedded
            ? ordered.find((source) => source.items.some(pickable))
            : undefined
          )?.id ??
            ordered[0]?.id ??
            "",
        );
      })
      .catch(() => {
        if (active) setError("providerImport.failed");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [open, sourcePath, embedded]);
  const current = sources.find((s) => s.id === tab);
  const mine = current?.items.filter(pickable) ?? [];
  const selected = mine.filter((it) => choices[keyOf(tab, it.reference)]?.on);
  const count = selected.length;
  useEffect(() => {
    if (allBox.current)
      allBox.current.indeterminate = count > 0 && count < mine.length;
  }, [count, mine.length]);
  useEffect(() => {
    document
      .getElementById(`import-tab-${tab}`)
      ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [tab]);
  function update(id: string, patch: Partial<Choice>) {
    setChoices((previous) => ({
      ...previous,
      [id]: { ...previous[id], ...patch },
    }));
  }
  async function submit() {
    if (saving.current || !count) return;
    saving.current = true;
    setBusy(true);
    setError("");
    const picks: ProviderImportPick[] = selected.map((item) => {
      const choice = choices[keyOf(tab, item.reference)];
      return {
        source: tab,
        reference: item.reference,
        fingerprint: item.fingerprint,
        mode: choice.mode,
        ...(choice.name?.trim() ? { name: choice.name.trim() } : {}),
      };
    });
    try {
      if (sourcePath) await providerImportApi.import(picks, sourcePath);
      else await providerImportApi.import(picks);
      await onImported();
      if (!embedded) onClose();
    } catch (cause) {
      setError(
        cause === "providerImport.conflict"
          ? "providerImport.conflict"
          : "providerImport.failed",
      );
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  async function browse() {
    setBusy(true);
    try {
      const path = await chooseFile({
        multiple: false,
        filters: [
          {
            name: "providers.json / logins.json",
            extensions: ["json"],
          },
        ],
      });
      if (typeof path === "string") setSourcePath(path);
    } catch {
      setError("providerImport.failed");
    } finally {
      setBusy(false);
    }
  }
  const content = (
    <>
      <header className="pv-import-title">
        {embedded ? (
          <h2>{t("providerImport.title")}</h2>
        ) : (
          <DialogTitle>{t("providerImport.title")}</DialogTitle>
        )}
      </header>
      {embedded ? (
        <p className="pv-import-note">{t("providerImport.readOnly")}</p>
      ) : (
        <DialogDescription className="pv-import-note">
          {t("providerImport.readOnly")}
        </DialogDescription>
      )}
      {loading ? (
        <p className="pv-import-empty" role="status">
          <Loader2 size={16} className="animate-spin" />
          {t("providerImport.loading")}
        </p>
      ) : (
        <>
          {!!sources.length && (
            <div
              role="tablist"
              aria-label={t("providerImport.sources")}
              className="pv-import-tabs"
            >
              {sources.map((source, index) => (
                <button
                  key={source.id}
                  type="button"
                  role="tab"
                  id={`import-tab-${source.id}`}
                  aria-controls={`import-panel-${source.id}`}
                  aria-selected={tab === source.id}
                  tabIndex={tab === source.id ? 0 : -1}
                  disabled={busy}
                  onClick={() => {
                    setTab(source.id);
                    scroll.current?.scrollTo?.(0, 0);
                  }}
                  onKeyDown={(event) => {
                    if (
                      !["ArrowRight", "ArrowLeft", "Home", "End"].includes(
                        event.key,
                      )
                    )
                      return;
                    event.preventDefault();
                    const at =
                      event.key === "Home"
                        ? 0
                        : event.key === "End"
                          ? sources.length - 1
                          : (index +
                              (event.key === "ArrowRight" ? 1 : -1) +
                              sources.length) %
                            sources.length;
                    setTab(sources[at].id);
                    document
                      .getElementById(`import-tab-${sources[at].id}`)
                      ?.focus();
                  }}
                >
                  <ProviderImportSourceIcon
                    sourceId={source.id}
                    name={source.name}
                  />
                  <span>{source.name}</span>
                  <span className="pv-import-count">
                    {source.found && !source.error
                      ? source.items.filter(pickable).length
                      : "–"}
                  </span>
                </button>
              ))}
            </div>
          )}
          {current && (
            <div
              ref={scroll}
              className="pv-import-list"
              role="tabpanel"
              id={`import-panel-${tab}`}
              aria-labelledby={`import-tab-${tab}`}
            >
              <div className="pv-import-source">
                <code title={current.path}>{current.path}</code>
                {tab === "magpie" && (
                  <button
                    type="button"
                    className="pv-import-browse"
                    disabled={busy}
                    onClick={() => void browse()}
                  >
                    <FolderOpen size={14} />
                    {t("providerImport.chooseFile")}
                  </button>
                )}
                {!!mine.length && (
                  <label>
                    <input
                      ref={allBox}
                      type="checkbox"
                      aria-label={t("providerEditor.selectAll")}
                      disabled={busy}
                      checked={count === mine.length}
                      onChange={(event) => {
                        const on = event.target.checked;
                        setChoices((previous) => {
                          const next = { ...previous };
                          for (const item of mine) {
                            const id = keyOf(tab, item.reference);
                            next[id] = { ...next[id], on };
                          }
                          return next;
                        });
                      }}
                    />
                    {t("providerEditor.selectAll")}
                  </label>
                )}
              </div>
              {tab === "magpie" && current.found && !current.error && (
                <p className="pv-import-note">
                  {t("providerImport.magpieScope")}
                </p>
              )}
              {!current.found ? (
                <p className="pv-import-empty">{t("providerImport.missing")}</p>
              ) : current.error ? (
                <p className="pv-import-empty">
                  {t("providerImport.sourceFailed")}
                </p>
              ) : !current.items.length ? (
                <p className="pv-import-empty">{t("providerImport.empty")}</p>
              ) : (
                current.items.map((item) => {
                  const id = keyOf(tab, item.reference);
                  const choice = choices[id];
                  const tag = item.skip
                    ? "unavailable"
                    : item.status === "same"
                      ? "added"
                      : item.status === "vendor"
                        ? "sameVendor"
                        : item.status === "taken"
                          ? "nameTaken"
                          : "";
                  return (
                    <div
                      key={item.reference}
                      className={`pv-import-row${!pickable(item) ? " pv-import-dim" : ""}`}
                    >
                      <input
                        id={`import-pick-${encodeURIComponent(id)}`}
                        type="checkbox"
                        aria-label={item.name}
                        disabled={busy || !pickable(item)}
                        checked={!!choice?.on}
                        onChange={(event) =>
                          update(id, { on: event.target.checked })
                        }
                      />
                      <Box size={17} aria-hidden className="pv-import-icon" />
                      <div className="pv-import-who">
                        <div className="pv-import-name">
                          <label
                            htmlFor={`import-pick-${encodeURIComponent(id)}`}
                          >
                            {item.name}
                          </label>
                          <small>{item.from}</small>
                        </div>
                        <div className="pv-import-sub">
                          {[
                            item.host,
                            t("providerImport.models", {
                              count: item.modelCount,
                            }),
                          ]
                            .filter(Boolean)
                            .join(" · ")}
                        </div>
                        {item.skip && (
                          <div className="pv-import-sub">
                            {t(
                              [
                                "subscription",
                                "reauth",
                                "selfRoute",
                                "missingUrl",
                                "missingKey",
                                "unresolved",
                                "unsupported",
                              ].includes(item.skip)
                                ? `providerImport.reasons.${item.skip}`
                                : "providerImport.reasons.unsupported",
                            )}
                          </div>
                        )}
                        {item.subscription && pickable(item) && (
                          <div className="pv-import-sub">
                            {t("providerImport.subscriptionWarning")}
                          </div>
                        )}
                        {!item.enabled && pickable(item) && (
                          <div className="pv-import-sub">
                            {t("providerImport.disabled")}
                          </div>
                        )}
                        {item.existing && (
                          <div className="pv-import-sub">
                            {t("providerImport.existing", {
                              name: item.existing,
                            })}
                          </div>
                        )}
                        {item.status === "taken" && choice && (
                          <Input
                            className="pv-import-rename"
                            aria-label={t("modelProviders.name")}
                            value={choice.name ?? item.name}
                            disabled={busy}
                            maxLength={120}
                            onChange={(event) =>
                              update(id, {
                                name: event.target.value,
                                on: true,
                              })
                            }
                          />
                        )}
                        {item.status === "vendor" && choice && (
                          <div
                            role="radiogroup"
                            aria-label={t("providerImport.sameVendor")}
                            className="pv-import-modes"
                          >
                            {(["key", "add"] as const).map((mode) => (
                              <button
                                key={mode}
                                type="button"
                                role="radio"
                                aria-checked={choice.mode === mode}
                                disabled={busy}
                                onClick={() => update(id, { mode, on: true })}
                              >
                                {t(
                                  mode === "key"
                                    ? "providerImport.asKey"
                                    : "providerImport.asNew",
                                )}
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                      {tag && (
                        <span className="pv-import-tag">
                          {t(`providerImport.${tag}`)}
                        </span>
                      )}
                    </div>
                  );
                })
              )}
            </div>
          )}
        </>
      )}
      {error && (
        <div className="pv-import-error" role="alert">
          {t(error)}
        </div>
      )}
      <footer className="pv-import-footer">
        <span>{t("providerImport.selected", { count })}</span>
        <Button variant="ghost" disabled={busy} onClick={onClose}>
          {t("common.cancel")}
        </Button>
        <Button
          disabled={loading || busy || !count}
          onClick={() => void submit()}
        >
          {busy && <Loader2 size={14} className="animate-spin" />}
          {t("providerFlow.import")}
        </Button>
      </footer>
    </>
  );
  if (embedded)
    return open ? (
      <section className="pv-import-inline">{content}</section>
    ) : null;
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value && !busy) onClose();
      }}
    >
      <DialogContent
        className="pv-import-dialog"
        onOpenAutoFocus={() => {
          returnFocus.current =
            document.activeElement instanceof HTMLElement
              ? document.activeElement
              : null;
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          const trigger = returnFocus.current?.isConnected
            ? returnFocus.current
            : document.querySelector<HTMLElement>(
                "[data-provider-add-trigger]",
              );
          trigger?.focus({ preventScroll: true });
        }}
        onEscapeKeyDown={(event) => {
          if (busy) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          if (busy) event.preventDefault();
        }}
      >
        {content}
      </DialogContent>
    </Dialog>
  );
}
