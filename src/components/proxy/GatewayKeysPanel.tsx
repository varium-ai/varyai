import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  gatewayApi,
  type GatewayKey,
  type GatewayPolicy,
} from "@/lib/api/gateway";

export function GatewayKeysPanel({ port }: { port: string }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const { data: keys = [], isError } = useQuery({
    queryKey: ["gateway-keys"],
    queryFn: gatewayApi.list,
    refetchInterval: 5000,
  });
  const [name, setName] = useState("");
  const [models, setModels] = useState("");
  const [limit, setLimit] = useState("");
  const [period, setPeriod] = useState<GatewayPolicy["period"]>("daily");
  const [editing, setEditing] = useState<string | null>(null);
  const [secret, setSecret] = useState("");
  const [pending, setPending] = useState(false);
  const valid =
    name.trim() &&
    models.trim() &&
    (limit === "" ||
      (/^\d+$/.test(limit) && Number.isSafeInteger(Number(limit))));
  async function run(action: () => Promise<unknown>) {
    setPending(true);
    try {
      await action();
      await client.invalidateQueries({ queryKey: ["gateway-keys"] });
    } catch {
      toast.error(t("gatewayKeys.failed"));
    } finally {
      setPending(false);
    }
  }
  function edit(key: GatewayKey) {
    setEditing(key.id);
    setName(key.policy.name);
    setModels(key.policy.models.join(", "));
    setLimit(
      key.policy.tokenLimit === null ? "" : String(key.policy.tokenLimit),
    );
    setPeriod(key.policy.period);
    setSecret("");
  }
  return (
    <section className="rounded-lg border border-border p-4 space-y-3">
      <h4 className="text-sm font-semibold">{t("gatewayKeys.title")}</h4>
      <p className="text-xs text-muted-foreground">
        {t("gatewayKeys.description")}
      </p>
      <code className="block text-xs break-all">
        http://127.0.0.1:{port}/gateway/v1
      </code>
      <p className="text-xs text-muted-foreground">
        {t("gatewayKeys.boundaries")}
      </p>
      {isError && <p role="alert">{t("gatewayKeys.failed")}</p>}
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid || pending) return;
          const policy: GatewayPolicy = {
            name: name.trim(),
            models: [
              ...new Set(
                models
                  .split(",")
                  .map((m) => m.trim())
                  .filter(Boolean),
              ),
            ],
            tokenLimit: limit === "" ? null : Number(limit),
            period,
          };
          void run(async () => {
            if (editing) await gatewayApi.update(editing, policy);
            else {
              const created = await gatewayApi.create(policy);
              setSecret(created.secret);
            }
            setEditing(null);
            setName("");
            setModels("");
            setLimit("");
          });
        }}
      >
        <div>
          <Label htmlFor="gateway-name">{t("gatewayKeys.name")}</Label>
          <Input
            id="gateway-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={120}
          />
        </div>
        <div>
          <Label htmlFor="gateway-models">{t("gatewayKeys.models")}</Label>
          <Input
            id="gateway-models"
            value={models}
            onChange={(e) => setModels(e.target.value)}
            required
          />
        </div>
        <div>
          <Label htmlFor="gateway-limit">{t("gatewayKeys.limit")}</Label>
          <Input
            id="gateway-limit"
            inputMode="numeric"
            value={limit}
            onChange={(e) => setLimit(e.target.value)}
          />
        </div>
        <div>
          <Label htmlFor="gateway-period">{t("gatewayKeys.period")}</Label>
          <select
            id="gateway-period"
            className="w-full h-9 rounded border bg-background px-2"
            value={period}
            onChange={(e) =>
              setPeriod(e.target.value as GatewayPolicy["period"])
            }
          >
            {(["never", "daily", "monthly"] as const).map((p) => (
              <option key={p} value={p}>
                {t(`gatewayKeys.${p}`)}
              </option>
            ))}
          </select>
        </div>
        <Button type="submit" disabled={!valid || pending}>
          {t(editing ? "common.save" : "gatewayKeys.create")}
        </Button>
        {editing && (
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              setEditing(null);
              setName("");
              setModels("");
              setLimit("");
            }}
          >
            {t("common.cancel")}
          </Button>
        )}
      </form>
      {secret && (
        <div className="space-y-2" role="status">
          <p className="text-xs">{t("gatewayKeys.once")}</p>
          <code className="block break-all select-all text-xs">{secret}</code>
          <Button variant="outline" onClick={() => setSecret("")}>
            {t("gatewayKeys.dismiss")}
          </Button>
        </div>
      )}
      {keys.map((key) => (
        <div key={key.id} className="border-t pt-3 space-y-1 text-xs">
          <p className="font-medium">
            {key.policy.name} {key.revoked && t("gatewayKeys.revoked")}
          </p>
          <p>{key.policy.models.join(", ")}</p>
          <p>
            {t("gatewayKeys.usage", {
              used: key.usedTokens,
              limit: key.policy.tokenLimit ?? "∞",
            })}
          </p>
          <p>
            {t("gatewayKeys.reset", {
              time: key.resetsAt
                ? new Date(key.resetsAt * 1000).toLocaleString()
                : t("gatewayKeys.never"),
            })}
          </p>
          {!key.revoked && (
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={pending}
                onClick={() => edit(key)}
              >
                {t("gatewayKeys.edit")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={pending}
                onClick={() => void run(() => gatewayApi.revoke(key.id))}
              >
                {t("gatewayKeys.revoke")}
              </Button>
            </div>
          )}
        </div>
      ))}
    </section>
  );
}
