import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api/settings";

interface SetupStatus {
  target: string;
  needsSetup: boolean;
}

export function StorageSetup({ errorKey }: { errorKey?: string }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SetupStatus>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(
    errorKey &&
      errorKey !== "storage.setupRequired" &&
      /^storage\.[a-zA-Z]+$/.test(errorKey)
      ? errorKey
      : "",
  );

  useEffect(() => {
    void invoke<SetupStatus>("get_storage_setup")
      .then(setStatus)
      .catch((cause) =>
        setError(
          typeof cause === "string" && /^storage\.[a-zA-Z]+$/.test(cause)
            ? cause
            : "storage.setupReadFailed",
        ),
      );
  }, []);

  const run = async (action: "create" | "choose" | "default") => {
    setBusy(true);
    setError("");
    try {
      if (action === "create") {
        await invoke("complete_storage_setup");
      } else if (action === "choose") {
        const path = await settingsApi.pickDirectory();
        if (!path) return;
        await settingsApi.setAppConfigDirOverride(path);
      } else {
        await settingsApi.setAppConfigDirOverride(null);
      }
      await settingsApi.restart();
    } catch (cause) {
      setError(
        typeof cause === "string" && /^storage\.[a-zA-Z]+$/.test(cause)
          ? cause
          : "storage.writeFailed",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <section className="w-full max-w-xl space-y-5 rounded-xl border bg-card p-6">
        <h1 className="text-xl font-semibold">{t("storage.title")}</h1>
        <p className="text-sm text-muted-foreground">
          {t("storage.description")}
        </p>
        {status && (
          <p className="break-all text-sm">
            {t("storage.destination")}: {status.target}
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {t(error, { defaultValue: t("storage.writeFailed") })}
          </p>
        )}
        <div className="flex flex-wrap gap-3">
          <Button
            disabled={busy || !status?.needsSetup}
            onClick={() => void run("create")}
          >
            {t(busy ? "storage.working" : "storage.startEmpty")}
          </Button>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void run("choose")}
          >
            {t("storage.useExisting")}
          </Button>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void run("default")}
          >
            {t("storage.useDefault")}
          </Button>
        </div>
      </section>
    </main>
  );
}
