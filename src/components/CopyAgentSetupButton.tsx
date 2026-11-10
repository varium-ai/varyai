import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";

export function CopyAgentSetupButton({
  labelKey = "common.copy",
  className,
  size,
  onCopied,
  onCopyFailed,
}: {
  labelKey?: "common.copy" | "onboarding.copyForAgent";
  className?: string;
  size?: "sm";
  onCopied: () => void;
  onCopyFailed: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => () => clearTimeout(resetTimer.current), []);

  return (
    <Button
      type="button"
      variant="outline"
      className={className}
      size={size}
      onClick={() => {
        void copyText(t("onboarding.helpPrompt"))
          .then(() => {
            setCopied(true);
            onCopied();
            clearTimeout(resetTimer.current);
            resetTimer.current = setTimeout(() => setCopied(false), 2000);
          })
          .catch(onCopyFailed);
      }}
    >
      {copied ? (
        <Check size={15} aria-hidden />
      ) : (
        <Copy size={15} aria-hidden />
      )}
      {t(copied ? "common.copied" : labelKey)}
    </Button>
  );
}
