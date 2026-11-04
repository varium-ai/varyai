import { useEffect, useState } from "react";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { useTranslation } from "react-i18next";

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  variant?: "destructive" | "info";
  zIndex?: "base" | "nested" | "alert" | "top";
  className?: string;
  /** 可选勾选项：提供 label 即显示，勾选状态经 onConfirm 参数回传 */
  checkboxLabel?: string;
  checkboxDefaultChecked?: boolean;
  pending?: boolean;
  onConfirm: (checkboxChecked: boolean) => void;
  onCancel: () => void;
  onExited?: () => void;
}

export function ConfirmDialog({
  isOpen,
  title,
  message,
  confirmText,
  cancelText,
  variant = "destructive",
  zIndex = "alert",
  className,
  checkboxLabel,
  checkboxDefaultChecked = false,
  pending = false,
  onConfirm,
  onCancel,
  onExited,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const [checkboxChecked, setCheckboxChecked] = useState(
    checkboxDefaultChecked,
  );

  useEffect(() => {
    if (isOpen) {
      setCheckboxChecked(checkboxDefaultChecked);
    }
  }, [isOpen, checkboxDefaultChecked]);

  return (
    <AppDialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !pending) {
          onCancel();
        }
      }}
      title={title}
      description={message}
      pending={pending}
      className={className}
      zIndex={zIndex}
      onCloseAutoFocus={onExited}
      footer={
        <>
          <Button variant="outline" onClick={onCancel} disabled={pending}>
            {cancelText || t("common.cancel")}
          </Button>
          <Button
            variant={variant === "info" ? "default" : "destructive"}
            disabled={pending}
            onClick={() => onConfirm(checkboxLabel ? checkboxChecked : false)}
          >
            {confirmText || t("common.confirm")}
          </Button>
        </>
      }
    >
      {checkboxLabel ? (
        <label className="vary-dialog-checkbox cursor-pointer select-none">
          <Checkbox
            checked={checkboxChecked}
            disabled={pending}
            onCheckedChange={(value) => setCheckboxChecked(value === true)}
            className="mt-0.5"
          />
          <span>{checkboxLabel}</span>
        </label>
      ) : null}
    </AppDialog>
  );
}
