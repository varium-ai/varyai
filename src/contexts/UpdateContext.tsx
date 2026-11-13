import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useRef,
} from "react";
import type { UpdateInfo, UpdateInstallProgress } from "../lib/updater";
import { checkForUpdate } from "../lib/updater";
import { extractErrorMessage } from "../utils/errorUtils";
import { settingsApi } from "@/lib/api";

interface UpdateContextValue {
  // 更新状态
  hasUpdate: boolean;
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  isInstalling: boolean;
  installProgress: UpdateInstallProgress | null;
  isDownloadStalled: boolean;
  isCancelling: boolean;
  cancelUpdate: () => Promise<void>;
  error: string | null;

  // 提示状态
  isDismissed: boolean;
  dismissUpdate: () => void;

  // 操作方法
  checkUpdate: () => Promise<boolean>;
  installUpdate: () => Promise<boolean>;
  resetDismiss: () => void;
}

const UpdateContext = createContext<UpdateContextValue | undefined>(undefined);

export function UpdateProvider({ children }: { children: React.ReactNode }) {
  const DISMISSED_VERSION_KEY = "ccswitch:update:dismissedVersion";
  const LEGACY_DISMISSED_KEY = "dismissedUpdateVersion"; // 兼容旧键

  const [hasUpdate, setHasUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);
  const [installProgress, setInstallProgress] =
    useState<UpdateInstallProgress | null>(null);
  const [isDownloadStalled, setIsDownloadStalled] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const attemptRef = useRef<string | null>(null);
  const progressRef = useRef<UpdateInstallProgress | null>(null);
  const cancellingRef = useRef(false);
  useEffect(() => {
    setIsDownloadStalled(false);
    if (!isInstalling || installProgress?.phase !== "downloading") return;
    const timer = setTimeout(() => setIsDownloadStalled(true), 15000);
    return () => clearTimeout(timer);
  }, [isInstalling, installProgress]);

  const [error, setError] = useState<string | null>(null);
  const [isDismissed, setIsDismissed] = useState(false);

  // 从 localStorage 读取已关闭的版本
  useEffect(() => {
    const current = updateInfo?.availableVersion;
    if (!current) return;

    // 读取新键；若不存在，尝试迁移旧键
    let dismissedVersion = localStorage.getItem(DISMISSED_VERSION_KEY);
    if (!dismissedVersion) {
      const legacy = localStorage.getItem(LEGACY_DISMISSED_KEY);
      if (legacy) {
        localStorage.setItem(DISMISSED_VERSION_KEY, legacy);
        localStorage.removeItem(LEGACY_DISMISSED_KEY);
        dismissedVersion = legacy;
      }
    }

    setIsDismissed(dismissedVersion === current);
  }, [updateInfo?.availableVersion]);

  const isCheckingRef = useRef(false);
  const isInstallingRef = useRef(false);

  const checkUpdate = useCallback(async () => {
    if (isCheckingRef.current) return false;
    isCheckingRef.current = true;
    setIsChecking(true);
    setError(null);

    try {
      const result = await checkForUpdate({ timeout: 30000 });

      if (result.status === "available") {
        setHasUpdate(true);
        setUpdateInfo(result.info);

        // 检查是否已经关闭过这个版本的提醒
        let dismissedVersion = localStorage.getItem(DISMISSED_VERSION_KEY);
        if (!dismissedVersion) {
          const legacy = localStorage.getItem(LEGACY_DISMISSED_KEY);
          if (legacy) {
            localStorage.setItem(DISMISSED_VERSION_KEY, legacy);
            localStorage.removeItem(LEGACY_DISMISSED_KEY);
            dismissedVersion = legacy;
          }
        }
        setIsDismissed(dismissedVersion === result.info.availableVersion);
        return true; // 有更新
      } else {
        setHasUpdate(false);
        setUpdateInfo(null);
        setIsDismissed(false);
        return false; // 已是最新
      }
    } catch (err) {
      console.error("检查更新失败:", err);
      setError(extractErrorMessage(err) || "检查更新失败");
      setHasUpdate(false);
      throw err; // 抛出错误让调用方处理
    } finally {
      setIsChecking(false);
      isCheckingRef.current = false;
    }
  }, []);

  const installUpdate = useCallback(async () => {
    if (isInstallingRef.current || !hasUpdate) return false;
    isInstallingRef.current = true;
    setIsInstalling(true);
    setError(null);

    const requestId = crypto.randomUUID();
    attemptRef.current = requestId;
    const initial: UpdateInstallProgress = {
      phase: "checking",
      downloaded: 0,
      total: null,
    };
    progressRef.current = initial;
    setInstallProgress(initial);
    setIsDownloadStalled(false);
    try {
      const result = await settingsApi.installUpdateAndRestart(
        requestId,
        (progress) => {
          if (attemptRef.current !== requestId) return;
          progressRef.current = progress;
          setInstallProgress(progress);
          setIsDownloadStalled(false);
        },
      );
      if (result === "cancelled") return false;
      if (result === "up-to-date") {
        setHasUpdate(false);
        setUpdateInfo(null);
        setIsDismissed(false);
        return false;
      }

      setHasUpdate(false);
      setUpdateInfo(null);
      setIsDismissed(false);
      return true;
    } catch (err) {
      console.error("Application update installation failed");
      setError("UPDATE_INSTALL_FAILED");
      throw err;
    } finally {
      attemptRef.current = null;
      progressRef.current = null;
      cancellingRef.current = false;
      setInstallProgress(null);
      setIsDownloadStalled(false);
      setIsCancelling(false);
      setIsInstalling(false);
      isInstallingRef.current = false;
    }
  }, [hasUpdate]);

  const cancelUpdate = useCallback(async () => {
    const requestId = attemptRef.current;
    if (
      !requestId ||
      progressRef.current?.phase === "installing" ||
      cancellingRef.current
    )
      return;
    cancellingRef.current = true;
    setIsCancelling(true);
    try {
      const accepted = await settingsApi.cancelUpdateDownload(requestId);
      if (!accepted && attemptRef.current === requestId) {
        cancellingRef.current = false;
        setIsCancelling(false);
      }
    } catch (err) {
      if (attemptRef.current === requestId) {
        cancellingRef.current = false;
        setIsCancelling(false);
      }
      throw err;
    }
  }, []);

  const dismissUpdate = useCallback(() => {
    setIsDismissed(true);
    if (updateInfo?.availableVersion) {
      localStorage.setItem(DISMISSED_VERSION_KEY, updateInfo.availableVersion);
      // 清理旧键
      localStorage.removeItem(LEGACY_DISMISSED_KEY);
    }
  }, [updateInfo?.availableVersion]);

  const resetDismiss = useCallback(() => {
    setIsDismissed(false);
    localStorage.removeItem(DISMISSED_VERSION_KEY);
    localStorage.removeItem(LEGACY_DISMISSED_KEY);
  }, []);

  // 应用启动时自动检查更新
  useEffect(() => {
    // 延迟1秒后检查，避免影响启动体验
    const timer = setTimeout(() => {
      checkUpdate().catch(console.error);
    }, 1000);

    return () => clearTimeout(timer);
  }, [checkUpdate]);

  const value: UpdateContextValue = {
    hasUpdate,
    updateInfo,
    isChecking,
    isInstalling,
    installProgress,
    isDownloadStalled,
    isCancelling,
    cancelUpdate,
    error,
    isDismissed,
    dismissUpdate,
    checkUpdate,
    installUpdate,
    resetDismiss,
  };

  return (
    <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
  );
}

export function useUpdate() {
  const context = useContext(UpdateContext);
  if (!context) {
    throw new Error("useUpdate must be used within UpdateProvider");
  }
  return context;
}
