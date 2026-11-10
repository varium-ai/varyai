import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import {
  agentConnectionsApi,
  type AgentModel,
  type AgentModelVisibility as Visibility,
} from "@/lib/api/agentConnections";
import appIcon from "@/assets/icons/app-icon.png";
import { Popover, PopoverTrigger } from "@/components/ui/popover";

const shown = (policy: Visibility, id: string) =>
  policy.ids.includes(id) === policy.only;
export function AgentModelVisibility({
  app,
  name,
  connected,
  models,
  disabled,
  onBusyChange,
}: {
  app: string;
  name: string;
  connected: boolean;
  models: AgentModel[];
  disabled: boolean;
  onBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const key = ["agent-model-visibility", app];
  const policy = useQuery({
    queryKey: key,
    queryFn: () => agentConnectionsApi.modelVisibility(app),
  });
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState<Visibility | null>(null);
  const latest = useRef<Visibility | null>(null);
  const saved = useRef<Visibility | null>(null);
  const queue = useRef<Visibility[]>([]);
  const running = useRef(false);
  const mounted = useRef(true);
  const busyCallback = useRef(onBusyChange);
  busyCallback.current = onBusyChange;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!policy.data || running.current) return;
    latest.current = saved.current = policy.data;
    setValue(policy.data);
  }, [policy.data]);

  function change(next: Visibility) {
    if (disabled) return;
    latest.current = next;
    setValue(next);
    setError(null);
    queue.current.push(next);
    if (running.current) return;
    running.current = true;
    setSaving(true);
    busyCallback.current(true);
    // Serial snapshots prevent a slower earlier write from undoing a later click.
    void (async () => {
      try {
        while (queue.current.length) {
          const update = queue.current.shift()!;
          await agentConnectionsApi.setModelVisibility(app, update);
          saved.current = update;
          client.setQueryData(key, update);
        }
      } catch {
        queue.current = [];
        latest.current = saved.current;
        if (mounted.current) {
          setValue(saved.current);
          setError("agentConnections.failed");
        }
      } finally {
        running.current = false;
        if (mounted.current) setSaving(false);
        busyCallback.current(false);
        void client.invalidateQueries({ queryKey: ["agent-connections"] });
      }
    })();
  }

  const selected = value ? models.filter((m) => shown(value, m.id)) : [];
  const summary =
    selected.length === models.length
      ? t("agentConnections.allModels", { count: models.length })
      : t("agentConnections.shownModels", {
          shown: selected.length,
          total: models.length,
        });

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next && !value) return;
        setOpen(next);
        if (next) setError(null);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className="mp-agent-start"
          disabled={disabled || !value || policy.isError}
          aria-label={`${t("agentConnections.modelList")} ${name}`}
          title={policy.isError ? t("agentConnections.failed") : summary}
        >
          <span className="mp-agent-model-icon" aria-hidden>
            <img src={appIcon} alt="" width={14} height={14} />
          </span>
          <span>
            {policy.isError
              ? t("agentConnections.failed")
              : !value
                ? t("common.loading")
                : summary}
          </span>
          <ChevronDown size={11} />
        </button>
      </PopoverTrigger>
      <ModelPickerPopoverContent
        onDismiss={() => setOpen(false)}
        aria-label={t("agentConnections.modelListTitle", { agent: name })}
      >
        <SharedModelPicker
          models={models}
          selected={selected.map((model) => model.id)}
          multiple
          disabled={disabled}
          name={name}
          title={t("agentConnections.modelListTitle", { agent: name })}
          description={t("agentConnections.modelListDescription", {
            agent: name,
          })}
          descriptionTitle={t(
            connected
              ? "agentConnections.modelListHint"
              : "agentConnections.modelListDisconnectedHint",
          )}
          error={error}
          saving={saving}
          onPick={(id) => {
            const current = latest.current;
            if (!current) return;
            const ids = new Set(current.ids);
            ids.has(id) ? ids.delete(id) : ids.add(id);
            change({ ...current, ids: [...ids] });
          }}
          visibility={{
            only: value?.only ?? false,
            order: value?.order,
            onShownChange: (rows, visible) => {
              const current = latest.current;
              if (!current) return;
              const ids = new Set(current.ids);
              for (const id of rows) {
                if (visible === current.only) ids.add(id);
                else ids.delete(id);
              }
              change({ ...current, ids: [...ids] });
            },
            onOnlyChange: (only) => {
              const current = latest.current;
              if (!current) return;
              change({
                ...current,
                only,
                ids: models
                  .filter((m) => shown(current, m.id) === only)
                  .map((m) => m.id),
              });
            },
            onOrderChange: (order) => {
              const current = latest.current;
              if (!current) return;
              const { order: _order, ...rest } = current;
              change(order.length ? { ...rest, order } : rest);
            },
          }}
        />
      </ModelPickerPopoverContent>
    </Popover>
  );
}
