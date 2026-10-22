import { useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { generateUUID } from "@/utils/uuid";
import {
  providerRequestsApi,
  type ProviderRequest,
} from "@/lib/api/providerRequests";

interface ProviderRequestDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ProviderRequestDialog({
  open,
  onOpenChange,
}: ProviderRequestDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [websiteUrl, setWebsiteUrl] = useState("");
  const [description, setDescription] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<{
    name?: string;
    websiteUrl?: string;
  }>({});
  const submitting = useRef(false);
  const lastRequest = useRef<ProviderRequest | null>(null);

  const validateName = (value: string) => {
    const name = value.trim();
    if (!name)
      return t("providerRequest.nameRequired", {
        defaultValue: "请输入供应商名称",
      });
    if ([...name].length > 50)
      return t("providerRequest.nameTooLong", {
        defaultValue: "供应商名称不能超过 50 个字符",
      });
    if (/[\p{Cc}]/u.test(name))
      return t("providerRequest.nameInvalid", {
        defaultValue: "供应商名称不能包含控制字符",
      });
    return undefined;
  };

  const validateWebsite = (value: string) => {
    const website = value.trim();
    if (!website)
      return t("providerRequest.websiteRequired", {
        defaultValue: "请输入供应商官网",
      });
    const invalid = () =>
      t("providerRequest.websiteInvalid", {
        defaultValue: "请输入有效的 http:// 或 https:// 官网链接",
      });
    const authority = website.match(/^https?:\/\/([^/?#]+)/i)?.[1];
    if (
      !authority ||
      authority.includes("@") ||
      [...website].length > 300 ||
      /[\s\\\p{Cc}]/u.test(website)
    )
      return invalid();
    try {
      const url = new URL(website);
      if (
        !["http:", "https:"].includes(url.protocol) ||
        !url.hostname ||
        url.username ||
        url.password
      )
        return invalid();
    } catch {
      return invalid();
    }
    return undefined;
  };

  const close = () => {
    if (!submitting.current) onOpenChange(false);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();

    event.stopPropagation();
    if (submitting.current) return;
    const request = {
      name: name.trim(),
      websiteUrl: websiteUrl.trim(),
      description: description.trim(),
    };
    const errors = {
      name: validateName(request.name),
      websiteUrl: validateWebsite(request.websiteUrl),
    };
    setFieldErrors(errors);
    setError(null);
    if (errors.name || errors.websiteUrl) return;
    if (
      [...request.description].length > 1000 ||
      /[\p{Cc}]/u.test(request.description.replace(/[\n\t]/g, ""))
    ) {
      setError(
        t("providerRequest.invalid", {
          defaultValue: "请填写供应商名称和有效的官网地址",
        }),
      );
      return;
    }
    submitting.current = true;
    setPending(true);
    setError(null);
    try {
      const previous = lastRequest.current;
      const payload =
        previous &&
        previous.name === request.name &&
        previous.websiteUrl === request.websiteUrl &&
        previous.description === request.description
          ? previous
          : { ...request, requestId: generateUUID() };
      lastRequest.current = payload;
      await providerRequestsApi.submit(payload);
      lastRequest.current = null;
      setName("");
      setWebsiteUrl("");
      setDescription("");
      setFieldErrors({});
      toast.success(
        t("providerRequest.success", {
          defaultValue: "申请已提交，感谢你的建议",
        }),
      );
      onOpenChange(false);
    } catch (reason) {
      const failure =
        typeof reason === "object" && reason !== null && "code" in reason
          ? (reason as { code: unknown; retryAfterSeconds?: unknown })
          : { code: reason };
      const key =
        failure.code === "temporarily_unavailable"
          ? "unavailable"
          : failure.code === "invalid_request"
            ? "invalid"
            : failure.code === "request_id_conflict"
              ? "conflict"
              : failure.code === "request_too_large"
                ? "tooLarge"
                : failure.code === "rate_limited"
                  ? "rateLimited"
                  : "failed";
      const defaults = {
        unavailable: "服务暂不可用，请稍后重试",
        invalid: "请填写供应商名称和有效的官网地址",
        conflict: "此申请编号已用于其他内容，请修改申请后重新提交",
        tooLarge: "申请内容过长，请缩短后重试",
        rateLimited: "提交过于频繁，请稍后重试",
        failed: "发送失败，请检查网络后重试",
      };
      const seconds = failure.retryAfterSeconds;
      setError(
        key === "rateLimited" &&
          typeof seconds === "number" &&
          Number.isSafeInteger(seconds) &&
          seconds > 0
          ? t("providerRequest.rateLimitedRetry", {
              seconds,
              defaultValue: "提交过于频繁，请在 {{seconds}} 秒后重试",
            })
          : t(`providerRequest.${key}`, { defaultValue: defaults[key] }),
      );
    } finally {
      submitting.current = false;
      setPending(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) close();
      }}
    >
      <DialogContent
        zIndex="top"
        className="max-w-md overflow-hidden"
        onEscapeKeyDown={(event) => {
          if (submitting.current) event.preventDefault();
        }}
      >
        <form onSubmit={submit} noValidate className="flex min-h-0 flex-col">
          <DialogHeader>
            <DialogTitle>
              {t("providerRequest.title", { defaultValue: "申请添加供应商" })}
            </DialogTitle>
            <DialogDescription>
              {t("providerRequest.description", {
                defaultValue:
                  "告诉我们你希望添加的供应商，申请将发送给团队评估。",
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 overflow-y-auto px-6 py-5">
            <p className="text-xs text-muted-foreground">
              {t("providerRequest.privacy", {
                defaultValue:
                  "提交时会附带应用版本和可获取的设备型号。请勿填写 API 密钥或隐私信息。",
              })}
            </p>
            <div className="space-y-2">
              <div className="flex items-center gap-1">
                <Label htmlFor="provider-request-name">
                  {t("providerRequest.name", { defaultValue: "供应商名称" })}
                </Label>
                <span aria-hidden="true" className="text-destructive">
                  *
                </span>
              </div>
              <Input
                id="provider-request-name"
                value={name}
                onChange={(event) => {
                  const value = event.target.value;
                  setName(value);
                  setFieldErrors((errors) => ({
                    ...errors,
                    name: undefined,
                  }));
                }}
                aria-invalid={!!fieldErrors.name}
                aria-describedby={
                  fieldErrors.name ? "provider-request-name-error" : undefined
                }
                className={
                  fieldErrors.name
                    ? "border-destructive focus:ring-destructive/20"
                    : undefined
                }
                placeholder={t("providerRequest.namePlaceholder", {
                  defaultValue: "例如：Example AI",
                })}
                maxLength={50}
                disabled={pending}
                required
              />
              {fieldErrors.name && (
                <p
                  id="provider-request-name-error"
                  role="alert"
                  className="text-sm text-destructive"
                >
                  {fieldErrors.name}
                </p>
              )}
            </div>
            <div className="space-y-2">
              <div className="flex items-center gap-1">
                <Label htmlFor="provider-request-website">
                  {t("providerRequest.website", { defaultValue: "供应商官网" })}
                </Label>
                <span aria-hidden="true" className="text-destructive">
                  *
                </span>
              </div>
              <Input
                id="provider-request-website"
                type="url"
                value={websiteUrl}
                onChange={(event) => {
                  const value = event.target.value;
                  setWebsiteUrl(value);
                  setFieldErrors((errors) => ({
                    ...errors,
                    websiteUrl: undefined,
                  }));
                }}
                aria-invalid={!!fieldErrors.websiteUrl}
                aria-describedby={
                  fieldErrors.websiteUrl
                    ? "provider-request-website-error"
                    : undefined
                }
                className={
                  fieldErrors.websiteUrl
                    ? "border-destructive focus:ring-destructive/20"
                    : undefined
                }
                placeholder={t("providerRequest.websitePlaceholder", {
                  defaultValue: "https://example.com",
                })}
                maxLength={300}
                disabled={pending}
                required
              />
              {fieldErrors.websiteUrl && (
                <p
                  id="provider-request-website-error"
                  role="alert"
                  className="text-sm text-destructive"
                >
                  {fieldErrors.websiteUrl}
                </p>
              )}
            </div>
            <div className="space-y-2">
              <Label htmlFor="provider-request-description">
                {t("providerRequest.details", {
                  defaultValue: "补充说明（选填）",
                })}
              </Label>
              <Textarea
                id="provider-request-description"
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                placeholder={t("providerRequest.detailsPlaceholder", {
                  defaultValue: "例如：希望支持该供应商的编码套餐",
                })}
                maxLength={1000}
                disabled={pending}
                rows={3}
              />
            </div>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={close}
              disabled={pending}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
            <Button
              type="submit"
              disabled={pending}
              aria-busy={pending}
              aria-label={
                pending
                  ? t("providerRequest.pending", { defaultValue: "提交中…" })
                  : undefined
              }
              className="relative"
            >
              <span className={pending ? "invisible" : undefined}>
                {t("providerRequest.submit", { defaultValue: "提交申请" })}
              </span>
              {pending && (
                <Loader2
                  className="absolute inset-0 m-auto h-4 w-4 animate-spin"
                  aria-hidden="true"
                />
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
