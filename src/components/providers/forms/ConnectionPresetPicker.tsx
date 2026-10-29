import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { ClaudeApiFormat } from "@/types";
import { ProviderConfigDetection } from "./ProviderConfigDetection";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  connectionPresetsApi,
  type ConnectionPreset,
} from "@/lib/api/connection-presets";

interface Props {
  onApply: (preset: ConnectionPreset) => void;
  current?: Partial<Omit<ConnectionPreset, "id">>;
  disabled?: boolean;
  supportedFormats?: ClaudeApiFormat[];
}

const formatLabels: Record<ClaudeApiFormat, string> = {
  anthropic: "providerForm.apiFormatAnthropic",
  openai_chat: "providerForm.apiFormatOpenAIChat",
  openai_responses: "providerForm.apiFormatOpenAIResponses",
  gemini_native: "providerForm.apiFormatGeminiNative",
};

export function ConnectionPresetPicker({
  onApply,
  current,
  disabled,
  supportedFormats,
}: Props) {
  const { t } = useTranslation();
  const id = useId();
  const manageButtonRef = useRef<HTMLButtonElement>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const [presets, setPresets] = useState<ConnectionPreset[]>([]);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState<
    "saveSuccess" | "deleteSuccess" | "newDraft" | null
  >(null);
  const [deleteTarget, setDeleteTarget] = useState<Pick<
    ConnectionPreset,
    "id" | "name"
  > | null>(null);
  const [testModel, setTestModel] = useState("");
  const [editingCredentials, setEditingCredentials] = useState(false);
  const [draft, setDraft] = useState<ConnectionPreset>({
    id: "",
    name: "",
    baseUrl: "",
    apiKey: "",
  });

  useEffect(() => {
    let active = true;
    connectionPresetsApi
      .list()
      .then((items) => {
        if (active) setPresets(items);
      })
      .catch(() => {
        if (active) setError("loadFailed");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const startNew = (fromCurrent = false) => {
    setDraft({
      id: crypto.randomUUID(),
      name: fromCurrent ? (current?.name ?? "") : "",
      baseUrl: fromCurrent ? (current?.baseUrl ?? "") : "",
      apiKey: fromCurrent ? (current?.apiKey ?? "") : "",
      ...(fromCurrent && current?.apiFormat
        ? { apiFormat: current.apiFormat }
        : {}),
    });
    setTestModel("");
    setError("");
    setSuccess(null);
  };

  const canApply = (preset: ConnectionPreset) =>
    !preset.apiFormat ||
    !supportedFormats ||
    supportedFormats.includes(preset.apiFormat);

  const save = async () => {
    if (pending) return;
    setSuccess(null);
    let endpoint: URL;
    try {
      endpoint = new URL(draft.baseUrl.trim());
      if (
        !draft.name.trim() ||
        !draft.apiKey.trim() ||
        !["https:", "http:"].includes(endpoint.protocol) ||
        endpoint.username ||
        endpoint.password ||
        endpoint.search ||
        endpoint.hash
      )
        throw new Error();
    } catch {
      setError("invalid");
      return;
    }
    const unchangedName =
      presets
        .find((preset) => preset.id === draft.id)
        ?.name.trim()
        .toLowerCase() === draft.name.trim().toLowerCase();
    if (
      !unchangedName &&
      presets.some(
        (preset) =>
          preset.id !== draft.id &&
          preset.name.trim().toLowerCase() === draft.name.trim().toLowerCase(),
      )
    ) {
      setError("nameExists");
      return;
    }
    setPending(true);
    setError("");
    try {
      const preset = {
        ...draft,
        name: draft.name.trim(),
        baseUrl: draft.baseUrl.trim(),
        apiKey: draft.apiKey.trim(),
      };
      await connectionPresetsApi.save(preset);
      setPresets((items) => [
        ...items.filter((item) => item.id !== preset.id),
        preset,
      ]);
      startNew();
      setSuccess("saveSuccess");
      toast.success(t("connectionPresets.saveSuccess"), { closeButton: true });
    } catch (error) {
      setError(
        String(error).includes("connectionPresets.nameExists")
          ? "nameExists"
          : "saveFailed",
      );
    } finally {
      setPending(false);
    }
  };

  const remove = async () => {
    if (pending || !deleteTarget) return;
    const presetId = deleteTarget.id;
    setPending(true);
    setError("");
    setSuccess(null);
    try {
      await connectionPresetsApi.remove(presetId);
      setPresets((items) => items.filter((item) => item.id !== presetId));
      if (draft.id === presetId) startNew();
      setSuccess("deleteSuccess");
      toast.success(t("connectionPresets.deleteSuccess"), {
        closeButton: true,
      });
    } catch {
      setError("deleteFailed");
      toast.error(t("connectionPresets.deleteFailed"), { closeButton: true });
    } finally {
      setPending(false);
      setDeleteTarget(null);
    }
  };

  return (
    <div className="space-y-2 rounded-lg border border-border-default p-3">
      <Label htmlFor={`${id}-select`}>{t("connectionPresets.label")}</Label>
      <div className="flex gap-2">
        <select
          id={`${id}-select`}
          value=""
          disabled={disabled || loading}
          className="h-9 min-w-0 flex-1 rounded-md border border-input bg-background px-3 text-sm disabled:opacity-50"
          onChange={(event) => {
            const preset = presets.find(
              (item) => item.id === event.target.value,
            );
            if (preset && canApply(preset)) onApply({ ...preset });
          }}
        >
          <option value="">
            {t(
              loading
                ? "connectionPresets.loading"
                : "connectionPresets.select",
            )}
          </option>
          {[...presets]
            .sort((a, b) => a.name.localeCompare(b.name))
            .map((preset) => (
              <option
                key={preset.id}
                value={preset.id}
                disabled={!canApply(preset)}
              >
                {preset.name}
                {!canApply(preset)
                  ? ` — ${t("connectionPresets.unsupportedFormat")}`
                  : ""}
              </option>
            ))}
        </select>
        <Button
          ref={manageButtonRef}
          type="button"
          variant="outline"
          disabled={loading}
          onClick={() => {
            startNew(true);
            setOpen(true);
          }}
        >
          {t("connectionPresets.manage")}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {t(
          disabled
            ? "connectionPresets.customRequired"
            : "connectionPresets.hint",
        )}
      </p>
      {error === "loadFailed" && !open && (
        <p className="text-xs text-destructive">
          {t("connectionPresets.loadFailed")}
        </p>
      )}
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!pending) {
            setOpen(value);
            if (!value) setDeleteTarget(null);
          }
        }}
      >
        <DialogContent
          zIndex="top"
          className="max-h-[85vh]"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            manageButtonRef.current?.focus();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("connectionPresets.manage")}</DialogTitle>
            <DialogDescription>{t("connectionPresets.hint")}</DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-4">
            <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>
                {t("connectionPresets.listCount", { count: presets.length })}
              </span>
              {presets.length > 4 && (
                <span>{t("connectionPresets.scrollHint")}</span>
              )}
            </div>
            <div
              role="region"
              aria-label={t("connectionPresets.list")}
              tabIndex={0}
              className="max-h-72 overflow-auto rounded-md border"
            >
              <table className="w-full text-left text-sm">
                <thead className="sticky top-0 z-10 bg-background">
                  <tr>
                    <th className="p-2 font-medium">
                      {t("connectionPresets.name")}
                    </th>
                    <th className="p-2 font-medium">
                      {t("providerForm.apiFormat")}
                    </th>
                    <th className="p-2 font-medium">{t("common.actions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {presets.map((preset) => (
                    <tr key={preset.id}>
                      <td className="max-w-40 truncate p-2" title={preset.name}>
                        {preset.name}
                      </td>
                      <td className="p-2">
                        {t(
                          preset.apiFormat
                            ? formatLabels[preset.apiFormat]
                            : "connectionPresets.keepFormat",
                        )}
                      </td>
                      <td className="whitespace-nowrap p-2">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          disabled={pending}
                          aria-label={`${t("connectionPresets.edit")} ${preset.name}`}
                          onClick={() => {
                            setDraft({ ...preset });
                            setTestModel("");
                            setError("");
                            setSuccess(null);
                          }}
                        >
                          {t("connectionPresets.edit")}
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          disabled={pending}
                          aria-label={`${t("connectionPresets.delete")} ${preset.name}`}
                          onClick={() =>
                            setDeleteTarget({
                              id: preset.id,
                              name: preset.name,
                            })
                          }
                        >
                          {t("connectionPresets.delete")}
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={() => {
                startNew();
                setSuccess("newDraft");
                nameInputRef.current?.focus();
              }}
            >
              {t("connectionPresets.add")}
            </Button>
            <div className="space-y-3">
              <div className="space-y-1">
                <Label htmlFor={`${id}-name`}>
                  {t("connectionPresets.name")}
                </Label>
                <Input
                  ref={nameInputRef}
                  id={`${id}-name`}
                  maxLength={80}
                  value={draft.name}
                  disabled={pending}
                  onChange={(event) =>
                    setDraft({ ...draft, name: event.target.value })
                  }
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor={`${id}-url`}>
                  {t("connectionPresets.baseUrl")}
                </Label>
                <Input
                  id={`${id}-url`}
                  value={draft.baseUrl}
                  disabled={pending}
                  placeholder="https://api.example.com/v1"
                  onFocus={() => setEditingCredentials(true)}
                  onBlur={() => setEditingCredentials(false)}
                  onChange={(event) =>
                    setDraft({ ...draft, baseUrl: event.target.value })
                  }
                />
                <p className="text-xs text-muted-foreground">
                  {t("connectionPresets.urlHint")}
                </p>
              </div>
              <div className="space-y-1">
                <Label htmlFor={`${id}-key`}>
                  {t("connectionPresets.apiKey")}
                </Label>
                <Input
                  id={`${id}-key`}
                  type="password"
                  autoComplete="off"
                  value={draft.apiKey}
                  disabled={pending}
                  onFocus={() => setEditingCredentials(true)}
                  onBlur={() => setEditingCredentials(false)}
                  onChange={(event) =>
                    setDraft({ ...draft, apiKey: event.target.value })
                  }
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor={`${id}-format`}>
                  {t("providerForm.apiFormat")}
                </Label>
                <select
                  id={`${id}-format`}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                  value={draft.apiFormat ?? ""}
                  disabled={pending}
                  onChange={(event) => {
                    const { apiFormat: _previous, ...rest } = draft;
                    setDraft(
                      event.target.value
                        ? {
                            ...rest,
                            apiFormat: event.target.value as ClaudeApiFormat,
                          }
                        : rest,
                    );
                  }}
                >
                  <option value="">{t("connectionPresets.keepFormat")}</option>
                  {Object.entries(formatLabels).map(([format, label]) => (
                    <option key={format} value={format}>
                      {t(label)}
                    </option>
                  ))}
                </select>
                <p className="text-xs text-muted-foreground">
                  {t("connectionPresets.formatHint")}
                </p>
              </div>
              <div className="space-y-1">
                <Label htmlFor={`${id}-test-model`}>
                  {t("connectionPresets.testModel")}
                </Label>
                <Input
                  id={`${id}-test-model`}
                  value={testModel}
                  disabled={pending}
                  onChange={(event) => setTestModel(event.target.value)}
                />
                <p className="text-xs text-muted-foreground">
                  {t("connectionPresets.testModelHint")}
                </p>
              </div>
              {open && (
                <ProviderConfigDetection
                  key={draft.id}
                  baseUrl={draft.baseUrl}
                  apiKey={draft.apiKey}
                  model={testModel}
                  apiFormat={draft.apiFormat}
                  editingCredentials={editingCredentials}
                  autoSelectFormat={false}
                  onModelChange={setTestModel}
                  onApiFormatChange={(apiFormat) =>
                    setDraft((previous) => ({ ...previous, apiFormat }))
                  }
                />
              )}
            </div>
          </div>
          <DialogFooter className="flex-row items-center">
            {error && (
              <p
                role="alert"
                className="mr-auto min-w-0 flex-1 text-sm text-destructive"
              >
                {t(`connectionPresets.${error}`)}
              </p>
            )}
            {success && (
              <p
                role="status"
                className="mr-auto text-sm font-medium text-emerald-600 dark:text-emerald-400"
              >
                {t(`connectionPresets.${success}`)}
              </p>
            )}
            <DialogClose asChild>
              <Button type="button" variant="outline" disabled={pending}>
                {t("common.close")}
              </Button>
            </DialogClose>
            <Button type="button" disabled={pending} onClick={save}>
              {t("connectionPresets.save")}
            </Button>
          </DialogFooter>
          {deleteTarget && (
            <ConfirmDialog
              isOpen
              zIndex="top"
              pending={pending}
              title={t("connectionPresets.deleteConfirmTitle")}
              message={t("connectionPresets.deleteConfirmMessage", {
                name: deleteTarget.name,
              })}
              confirmText={t("connectionPresets.delete")}
              onConfirm={() => void remove()}
              onCancel={() => {
                if (!pending) setDeleteTarget(null);
              }}
            />
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
