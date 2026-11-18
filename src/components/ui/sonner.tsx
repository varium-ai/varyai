import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";
import { useTranslation } from "react-i18next";

export function Toaster() {
  const { theme } = useTheme();
  const { t } = useTranslation();

  return (
    <SonnerToaster
      position="top-center"
      theme={theme}
      richColors
      style={
        {
          "--normal-bg": "hsl(var(--card))",
          "--normal-border": "hsl(var(--border))",
          "--normal-text": "hsl(var(--foreground))",
          "--success-bg": "var(--normal-bg)",
          "--success-border": "var(--normal-border)",
          "--success-text": "var(--success)",
          "--info-bg": "var(--normal-bg)",
          "--info-border": "var(--normal-border)",
          "--info-text": "hsl(var(--primary))",
          "--warning-bg": "var(--normal-bg)",
          "--warning-border": "var(--normal-border)",
          "--warning-text": "var(--warning)",
          "--error-bg": "var(--normal-bg)",
          "--error-border": "var(--normal-border)",
          "--error-text": "hsl(var(--destructive))",
        } as React.CSSProperties
      }
      toastOptions={{
        duration: 3200,
        closeButtonAriaLabel: t("common.close"),
        classNames: {
          toast:
            "group w-[calc(100vw-2rem)] max-w-[420px] items-start gap-3 rounded-md px-4 py-3 shadow-md",
          content: "min-w-0 flex-1",
          title: "text-sm font-semibold leading-5 text-foreground",
          description: "mt-1 text-xs leading-5 !text-secondary-foreground",
          icon: "mt-0.5 h-4 w-4 shrink-0 group-data-[type=loading]:text-primary",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 transition-colors",
          actionButton:
            "rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-[var(--primary-hover)]",
        },
      }}
    />
  );
}
