import { useId } from "react";
import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type {
  DirectoryAppId,
  ResolvedDirectories,
} from "@/hooks/useDirectorySettings";

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  codexDir?: string;
  geminiDir?: string;
  grokDir?: string;
  opencodeDir?: string;
  openclawDir?: string;
  hermesDir?: string;
  piDir?: string;
  workbuddyDir?: string;
  onDirectoryChange: (app: DirectoryAppId, value?: string) => void;
  onBrowseDirectory: (app: DirectoryAppId) => Promise<void>;
  onResetDirectory: (app: DirectoryAppId) => Promise<void>;
}

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  onAppConfigChange,
  onBrowseAppConfig,
  onResetAppConfig,
  claudeDir,
  codexDir,
  geminiDir,
  grokDir,
  opencodeDir,
  openclawDir,
  hermesDir,
  piDir,
  workbuddyDir,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();
  const appConfigInputId = useId();
  const appConfigDescriptionId = useId();

  return (
    <div className="settings-directory-form">
      <section className="settings-directory-section">
        <header className="settings-advanced-heading">
          <h3>
            <label htmlFor={appConfigInputId}>
              {t("settings.appConfigDir")}
            </label>
          </h3>
          <p id={appConfigDescriptionId}>
            {t("settings.appConfigDirDescription")}
          </p>
        </header>
        <DirectoryPathInput
          id={appConfigInputId}
          descriptionId={appConfigDescriptionId}
          value={appConfigDir ?? resolvedDirs.appConfig ?? ""}
          placeholder={t("settings.browsePlaceholderApp")}
          onChange={onAppConfigChange}
          onBrowse={onBrowseAppConfig}
          onReset={onResetAppConfig}
        />
      </section>

      <section className="settings-directory-section">
        <header className="settings-advanced-heading">
          <h3>{t("settings.configDirectoryOverride")}</h3>
          <p>{t("settings.configDirectoryDescription")}</p>
        </header>

        <div className="settings-directory-list">
          <DirectoryInput
            label={t("settings.claudeConfigDir")}
            value={claudeDir}
            resolvedValue={resolvedDirs.claude}
            placeholder={t("settings.browsePlaceholderClaude")}
            onChange={(val) => onDirectoryChange("claude", val)}
            onBrowse={() => onBrowseDirectory("claude")}
            onReset={() => onResetDirectory("claude")}
          />

          <DirectoryInput
            label={t("settings.codexConfigDir")}
            value={codexDir}
            resolvedValue={resolvedDirs.codex}
            placeholder={t("settings.browsePlaceholderCodex")}
            onChange={(val) => onDirectoryChange("codex", val)}
            onBrowse={() => onBrowseDirectory("codex")}
            onReset={() => onResetDirectory("codex")}
          />

          <DirectoryInput
            label={t("settings.geminiConfigDir")}
            value={geminiDir}
            resolvedValue={resolvedDirs.gemini}
            placeholder={t("settings.browsePlaceholderGemini")}
            onChange={(val) => onDirectoryChange("gemini", val)}
            onBrowse={() => onBrowseDirectory("gemini")}
            onReset={() => onResetDirectory("gemini")}
          />

          <DirectoryInput
            label={t("settings.grokConfigDir")}
            value={grokDir}
            resolvedValue={resolvedDirs.grokbuild}
            placeholder={t("settings.browsePlaceholderGrok")}
            onChange={(val) => onDirectoryChange("grokbuild", val)}
            onBrowse={() => onBrowseDirectory("grokbuild")}
            onReset={() => onResetDirectory("grokbuild")}
          />

          <DirectoryInput
            label={t("settings.opencodeConfigDir")}
            value={opencodeDir}
            resolvedValue={resolvedDirs.opencode}
            placeholder={t("settings.browsePlaceholderOpencode")}
            onChange={(val) => onDirectoryChange("opencode", val)}
            onBrowse={() => onBrowseDirectory("opencode")}
            onReset={() => onResetDirectory("opencode")}
          />

          <DirectoryInput
            label={t("settings.openclawConfigDir")}
            value={openclawDir}
            resolvedValue={resolvedDirs.openclaw}
            placeholder={t("settings.browsePlaceholderOpenclaw")}
            onChange={(val) => onDirectoryChange("openclaw", val)}
            onBrowse={() => onBrowseDirectory("openclaw")}
            onReset={() => onResetDirectory("openclaw")}
          />

          <DirectoryInput
            label={t("settings.hermesConfigDir")}
            value={hermesDir}
            resolvedValue={resolvedDirs.hermes}
            placeholder={t("settings.browsePlaceholderHermes")}
            onChange={(val) => onDirectoryChange("hermes", val)}
            onBrowse={() => onBrowseDirectory("hermes")}
            onReset={() => onResetDirectory("hermes")}
          />

          <DirectoryInput
            label={t("settings.piConfigDir")}
            value={piDir}
            resolvedValue={resolvedDirs.pi}
            placeholder={t("settings.browsePlaceholderPi")}
            onChange={(val) => onDirectoryChange("pi", val)}
            onBrowse={() => onBrowseDirectory("pi")}
            onReset={() => onResetDirectory("pi")}
          />
          <DirectoryInput
            label={t("settings.workbuddyConfigDir")}
            value={workbuddyDir}
            resolvedValue={resolvedDirs.workbuddy}
            placeholder={"~/.workbuddy-ai"}
            onChange={(val) => onDirectoryChange("workbuddy", val)}
            onBrowse={() => onBrowseDirectory("workbuddy")}
            onReset={() => onResetDirectory("workbuddy")}
          />
        </div>
      </section>
    </div>
  );
}

interface DirectoryPathInputProps {
  id: string;
  descriptionId?: string;
  value: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
  onReset: () => Promise<void>;
}

interface DirectoryInputProps
  extends Omit<DirectoryPathInputProps, "id" | "descriptionId" | "value"> {
  label: string;
  value?: string;
  resolvedValue: string;
}

function DirectoryInput({
  label,
  value,
  resolvedValue,
  ...props
}: DirectoryInputProps) {
  const id = useId();

  return (
    <div className="settings-directory-row">
      <label htmlFor={id}>{label}</label>
      <DirectoryPathInput
        id={id}
        value={value ?? resolvedValue ?? ""}
        {...props}
      />
    </div>
  );
}

function DirectoryPathInput({
  id,
  descriptionId,
  value,
  placeholder,
  onChange,
  onBrowse,
  onReset,
}: DirectoryPathInputProps) {
  const { t } = useTranslation();

  return (
    <div className="settings-directory-controls">
      <Input
        id={id}
        aria-describedby={descriptionId}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
      <Button
        type="button"
        variant="outline"
        size="icon"
        onClick={onBrowse}
        title={t("settings.browseDirectory")}
      >
        <FolderSearch aria-hidden="true" />
      </Button>
      <Button
        type="button"
        variant="outline"
        size="icon"
        onClick={onReset}
        title={t("settings.resetDefault")}
      >
        <Undo2 aria-hidden="true" />
      </Button>
    </div>
  );
}
