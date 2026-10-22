import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";
import { useTranslation } from "react-i18next";

export function Toaster() {
  const { theme } = useTheme();
  const { t } = useTranslation();

  // 将应用主题映射到 Sonner 的主题
  // 如果是 "system"，Sonner 会自己处理
  const sonnerTheme = theme === "system" ? "system" : theme;

  return (
    <SonnerToaster
      position="top-center"
      theme={sonnerTheme}
      richColors
      toastOptions={{
        duration: 3200,
        closeButtonAriaLabel: t("common.close"),
        classNames: {
          toast:
            "group w-[calc(100vw-2rem)] max-w-[420px] items-start gap-3 rounded-xl px-4 py-3 shadow-lg shadow-slate-900/10 backdrop-blur-md",
          content: "min-w-0 flex-1",
          title: "text-sm font-semibold leading-5",
          description: "mt-1 text-xs leading-5",
          icon: "mt-0.5 h-4 w-4 shrink-0",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 transition-colors",
          actionButton:
            "rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-blue-700",
        },
      }}
    />
  );
}
