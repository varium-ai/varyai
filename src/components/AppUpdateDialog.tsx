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
  fetchReleaseHistory,
  meaningfulReleaseNotes,
  localizedReleaseNotes,
  releaseNotesPageUrl,
} from "@/lib/releaseNotes";
import { Button } from "@/components/ui/button";
import { AppDialog } from "@/components/ui/app-dialog";
import "./app-update-dialog.css";

export function AppUpdateDialog() {
  const { t, i18n } = useTranslation();
  const {
    hasUpdate,
    updateInfo,
    isDismissed,
    isInstalling,
    installProgress,
    isDownloadStalled,
    isCancelling,
    cancelUpdate,
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
  const history = useQuery({
    queryKey: [
      "app-release-history",
      updateInfo?.currentVersion,
      updateInfo?.availableVersion,
    ],
    queryFn: ({ signal }) =>
      fetchReleaseHistory(
        updateInfo?.currentVersion ?? "",
        updateInfo?.availableVersion ?? "",
        signal,
      ),
    enabled:
      isOpen && isPortable !== null && Boolean(updateInfo?.currentVersion),
    staleTime: 15 * 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  if (isPortable === null || !updateInfo) return null;
  const targetVersion = updateInfo.availableVersion.replace(/^v/, "");
  const versionKey = (version: string) =>
    version.replace(/^v/, "").split("+")[0];
  const targetNotes =
    manifestNotes ||
    release.data ||
    history.data?.releases.find(
      (entry) => versionKey(entry.version) === versionKey(targetVersion),
    )?.notes ||
    "";
  const versions = [
    { version: targetVersion, notes: targetNotes },
    ...(history.data?.releases ?? []).filter(
      (entry) => versionKey(entry.version) !== versionKey(targetVersion),
    ),
  ];
  const displayed = versions.slice(0, 40).map((entry) => ({
    ...entry,
    notes: localizedReleaseNotes(
      entry.notes,
      i18n.resolvedLanguage || i18n.language,
    ),
  }));
  const historyIncomplete =
    history.isError || history.data?.complete === false || versions.length > 40;
  const hasNotes = displayed.some((entry) => entry.notes);
  const releaseUrl = releaseNotesPageUrl(updateInfo.availableVersion);

  const phase = installProgress?.phase;
  const canCancel = isInstalling && Boolean(phase) && phase !== "installing";
  const percent =
    installProgress?.total && installProgress.total > 0
      ? Math.min(
          100,
          Math.floor(
            (installProgress.downloaded / installProgress.total) * 100,
          ),
        )
      : undefined;
  const formatBytes = (bytes: number) => {
    const unit = bytes >= 1024 * 1024 ? "MB" : "KB";
    const value = bytes / (unit === "MB" ? 1024 * 1024 : 1024);
    return `${new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 1 }).format(value)} ${unit}`;
  };
  const handleCancel = async () => {
    try {
      await cancelUpdate();
    } catch {
      toast.error(t("settings.updateCancelFailed"));
    }
  };
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
          {isInstalling && phase && (
            <div className="vary-update-progress">
              <p role="status" className="vary-update-progress-label">
                {t(
                  isCancelling
                    ? "settings.updateCancelling"
                    : phase === "checking"
                      ? "settings.updatePreparing"
                      : phase === "installing"
                        ? "settings.updateInstalling"
                        : "settings.updateDownloading",
                )}
                {phase === "downloading" && installProgress && (
                  <span>
                    {formatBytes(installProgress.downloaded)}
                    {installProgress.total
                      ? ` / ${formatBytes(installProgress.total)} · ${percent}%`
                      : ""}
                  </span>
                )}
              </p>
              {phase === "downloading" && (
                <progress
                  aria-label={t("settings.updateDownloading")}
                  max={100}
                  value={percent}
                />
              )}
              {isDownloadStalled && phase === "downloading" && (
                <p role="status" className="vary-update-progress-hint">
                  {t("settings.updateDownloadStalled")}
                </p>
              )}
            </div>
          )}
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
            disabled={isInstalling && (!canCancel || isCancelling)}
            onClick={canCancel ? () => void handleCancel() : dismissUpdate}
          >
            {t(canCancel ? "settings.updateCancel" : "settings.updateLater")}
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
          <span>
            {updateInfo.currentVersion
              ? t("settings.releaseHistoryRange", {
                  from: updateInfo.currentVersion.replace(/^v/, ""),
                  to: targetVersion,
                })
              : t("settings.releaseNotes")}
          </span>
          <button type="button" onClick={() => void openExternal(releaseUrl)}>
            {t("settings.viewReleaseNotes")}
            <ArrowUpRight size={12} aria-hidden />
          </button>
        </div>
        {(history.isFetching || historyIncomplete) && (
          <p className="vary-update-history-status" role="status">
            {t(
              history.isFetching
                ? "settings.releaseHistoryLoading"
                : "settings.releaseHistoryIncomplete",
            )}
          </p>
        )}
        {hasNotes ? (
          displayed.map(({ version, notes }) => (
            <article
              className="vary-update-release"
              key={version}
              aria-label={`v${version}`}
            >
              <h3 className="vary-update-release-version">v{version}</h3>
              {notes ? (
                <div className="vary-update-markdown">
                  <ReactMarkdown
                    skipHtml
                    disallowedElements={["img"]}
                    components={{
                      h1: ({ children }) => <h4>{children}</h4>,
                      h2: ({ children }) => <h4>{children}</h4>,
                      h3: ({ children }) => <h5>{children}</h5>,
                      li: ({ children }) => (
                        <li>
                          {children}{" "}
                          <span className="vary-update-item-version">
                            v{version}
                          </span>
                        </li>
                      ),
                      a: ({ href, children, title }) => {
                        if (!href) return <span>{children}</span>;
                        let url: URL;
                        try {
                          url = new URL(href, releaseNotesPageUrl(version));
                        } catch {
                          return <span>{children}</span>;
                        }
                        if (
                          url.protocol !== "https:" &&
                          url.protocol !== "http:"
                        )
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
                </div>
              ) : (
                <p className="vary-update-history-status">
                  {t("settings.releaseNotesUnavailable")}
                </p>
              )}
            </article>
          ))
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
