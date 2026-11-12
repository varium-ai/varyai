import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import { ArrowUpRight, Download, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useUpdate } from "@/contexts/UpdateContext";
import { settingsApi } from "@/lib/api";
import { RELEASES_LATEST_URL } from "@/config/brand";
import {
  fetchReleaseNotes,
  meaningfulReleaseNotes,
  releaseNotesPageUrl,
} from "@/lib/releaseNotes";
import { Button } from "@/components/ui/button";
import { AppDialog } from "@/components/ui/app-dialog";
import "./app-update-dialog.css";

export function AppUpdateDialog() {
  const { t } = useTranslation();
  const {
    hasUpdate,
    updateInfo,
    isDismissed,
    isInstalling,
    dismissUpdate,
    installUpdate,
  } = useUpdate();
  const [isPortable, setIsPortable] = useState<boolean | null>(null);

  useEffect(() => {
    let active = true;
    void settingsApi
      .isPortable()
      .then((portable) => {
        if (active) setIsPortable(portable);
      })
      .catch(() => {
        if (active) setIsPortable(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const isOpen = hasUpdate && Boolean(updateInfo) && !isDismissed;
  const manifestNotes = meaningfulReleaseNotes(updateInfo?.notes);
  const release = useQuery({
    queryKey: ["app-release-notes", updateInfo?.availableVersion],
    queryFn: ({ signal }) =>
      fetchReleaseNotes(updateInfo?.availableVersion ?? "", signal),
    enabled: isOpen && isPortable !== null && !manifestNotes,
    staleTime: 15 * 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  if (isPortable === null || !updateInfo) return null;
  const notes = manifestNotes || release.data || "";
  const releaseUrl = releaseNotesPageUrl(updateInfo.availableVersion);

  const handleInstall = async () => {
    try {
      await installUpdate();
    } catch {
      toast.error(t("settings.updateInstallFailed"));
    }
  };
  const openExternal = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch {
      toast.error(t("settings.openReleaseNotesFailed"));
    }
  };

  return (
    <AppDialog
      open={isOpen}
      pending={isInstalling}
      onOpenChange={(open) => {
        if (!open && !isInstalling) dismissUpdate();
      }}
      zIndex="top"
      size="lg"
      className="vary-update-dialog"
      title={t("settings.updateTo", { version: updateInfo.availableVersion })}
      description={
        updateInfo.currentVersion
          ? t("settings.updateCurrentVersion", {
              version: updateInfo.currentVersion,
            })
          : undefined
      }
      footer={
        <>
          <p className="vary-update-footer-hint">
            {t(
              isPortable
                ? "settings.portableMode"
                : "settings.updateRestartHint",
            )}
          </p>
          <Button
            type="button"
            variant="outline"
            disabled={isInstalling}
            onClick={dismissUpdate}
          >
            {t("settings.updateLater")}
          </Button>
          {isPortable ? (
            <Button
              type="button"
              onClick={() => void openExternal(RELEASES_LATEST_URL)}
            >
              <Download className="mr-2 h-4 w-4" />
              {t("settings.downloadUpdate")}
            </Button>
          ) : (
            <Button
              type="button"
              disabled={isInstalling}
              onClick={() => void handleInstall()}
            >
              {isInstalling && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t(
                isInstalling ? "settings.updating" : "settings.restartToUpdate",
              )}
            </Button>
          )}
        </>
      }
    >
      <section
        className="vary-update-notes"
        aria-label={t("settings.releaseNotes")}
      >
        <div className="vary-update-notes-toolbar">
          <span>{t("settings.releaseNotes")}</span>
          <button type="button" onClick={() => void openExternal(releaseUrl)}>
            {t("settings.viewReleaseNotes")}
            <ArrowUpRight size={12} aria-hidden />
          </button>
        </div>
        {notes ? (
          <article className="vary-update-markdown">
            <ReactMarkdown
              skipHtml
              disallowedElements={["img"]}
              components={{
                h1: ({ children }) => <h3>{children}</h3>,
                h2: ({ children }) => <h3>{children}</h3>,
                h3: ({ children }) => <h4>{children}</h4>,
                a: ({ href, children, title }) => {
                  if (!href) return <span>{children}</span>;
                  let url: URL;
                  try {
                    url = new URL(href, releaseUrl);
                  } catch {
                    return <span>{children}</span>;
                  }
                  if (url.protocol !== "https:" && url.protocol !== "http:")
                    return <span>{children}</span>;
                  return (
                    <a
                      href={url.href}
                      title={title}
                      onClick={(event) => {
                        event.preventDefault();
                        void openExternal(url.href);
                      }}
                    >
                      {children}
                    </a>
                  );
                },
              }}
            >
              {notes}
            </ReactMarkdown>
          </article>
        ) : (
          <p className="vary-update-empty" role="status">
            {t(
              release.isFetching
                ? "common.loading"
                : "settings.releaseNotesUnavailable",
            )}
          </p>
        )}
      </section>
    </AppDialog>
  );
}
