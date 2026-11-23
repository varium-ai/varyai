import { useId, useRef, useState } from "react";
import { useDialogSnapshot } from "@/hooks/useDialogSnapshot";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronsUpDown,
  FolderCog,
  FolderOpen,
  Plus,
  X,
} from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api/types";
import {
  useClearProfileMutation,
  useCreateProfileMutation,
  useProfilesQuery,
} from "@/lib/query/profiles";
import { ProfileManageDialog } from "./ProfileManageDialog";
import { ProfileBundleDialog } from "./ProfileBundleDialog";
import { ProfileLegacyDialog } from "./ProfileLegacyDialog";
import { APP_PROFILE_SCOPE, hasScopeSnapshot } from "./scope";
import type {
  CurrentProfileIds,
  ProfileScope,
  Profile,
} from "@/lib/api/profiles";
import { useQueryClient } from "@tanstack/react-query";
import { APP_IDS } from "@/config/appConfig";
import { profilesApi, type ProfileUndo } from "@/lib/api/profiles";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { toast } from "sonner";
import { profileErrorMessage } from "@/utils/profileErrors";

const CURRENT_ID_KEY: Record<ProfileScope, keyof CurrentProfileIds> = {
  claude: "claude",
  "claude-desktop": "claudeDesktop",
  codex: "codex",
};

interface ProfileSwitcherProps {
  activeApp: AppId;
  selectedApps: AppId[];
  side?: "top" | "bottom";
  compact?: boolean;
  nativeAgentPicker?: boolean;
  triggerClassName?: string;
}

/** Shared project menu: legacy profiles apply to their scope; bundles preview all selected agents. */
export function ProfileSwitcher({
  activeApp,
  selectedApps,
  side = "bottom",
  compact = false,
  nativeAgentPicker = compact,
  triggerClassName,
}: ProfileSwitcherProps) {
  const { t } = useTranslation();
  const nameErrorId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const openingCreateDialog = useRef(false);
  const [open, setOpen] = useState(false);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isManageOpen, setIsManageOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [bundleApps, setBundleApps] = useState<AppId[]>([]);
  const [createBundle, setCreateBundle] = useState(false);
  const [captureApp, setCaptureApp] = useState<AppId>(activeApp);
  const [pendingUndo, setPendingUndo] = useState<ProfileUndo | null>(null);
  const [undoing, setUndoing] = useState(false);
  const [bundlePreview, setBundlePreview] = useState<Profile | null>(null);
  const [legacyPreview, setLegacyPreview] = useState<Profile | null>(null);
  const bundleDialog = useDialogSnapshot(bundlePreview);
  const legacyDialog = useDialogSnapshot(legacyPreview);
  const queryClient = useQueryClient();

  const { data } = useProfilesQuery();
  const clearMutation = useClearProfileMutation();
  const createMutation = useCreateProfileMutation();

  const scope = APP_PROFILE_SCOPE[activeApp];

  const profiles = (data?.profiles ?? []).filter(
    (p) => scope || p.payload.bundle,
  );
  const agents =
    data?.agents ??
    APP_IDS.map((app) => ({
      app,
      available: app !== "doubaowork",
      reason: null,
      mcp: false,
      skills: false,
      instructions: false,
      provider: app !== "doubaowork",
    }));
  const captureApps = agents
    .filter(
      (agent) =>
        agent.available &&
        (createBundle
          ? bundleApps.includes(agent.app)
          : agent.app === captureApp),
    )
    .map((agent) => agent.app);
  const selectedAvailableApps = agents
    .filter((agent) => agent.available && selectedApps.includes(agent.app))
    .map((agent) => agent.app);
  const currentAgent = agents.find((agent) => agent.app === captureApp);
  const scopedCurrentId =
    data?.currentIds?.byApp?.[activeApp] ??
    (scope ? data?.currentIds?.[CURRENT_ID_KEY[scope]] : null) ??
    null;
  const scopedProfile = profiles.find((p) => p.id === scopedCurrentId);
  const currentId =
    data?.currentBundleId ??
    (data?.currentBundleId !== undefined && scopedProfile?.payload.bundle
      ? null
      : scopedCurrentId);
  const currentProfile = profiles.find((p) => p.id === currentId);
  const duplicateName =
    !createMutation.isPending &&
    !createMutation.isSuccess &&
    Boolean(newName.trim()) &&
    (data?.profiles ?? []).some(
      (profile) => profile.name.trim() === newName.trim(),
    );

  const undoLast = async () => {
    if (!pendingUndo || undoing) return;
    setUndoing(true);
    try {
      await profilesApi.undoBundle(pendingUndo.id);
      await queryClient.invalidateQueries({ queryKey: ["profiles"] });
      setPendingUndo(null);
      toast.success(t("profiles.bundle.restored"));
    } catch (value) {
      const message = value instanceof Error ? value.message : String(value);
      const key = message.match(/profiles\.bundle\.[a-zA-Z]+/)?.[0];
      toast.error(t(key ?? "profiles.bundle.operationFailed"));
    } finally {
      setUndoing(false);
    }
  };
  const handleApply = (id: string) => {
    setOpen(false);
    const profile = profiles.find((p) => p.id === id);
    if (profile?.payload.bundle) {
      setBundlePreview(profile);
      return;
    }
    if (scope && profile) setLegacyPreview(profile);
  };

  const closeCreateDialog = () => {
    setIsCreateOpen(false);
    createMutation.reset();
    setNewName("");
    setBundleApps([]);
    setCreateBundle(false);
    setCaptureApp(activeApp);
  };

  const handleCreate = () => {
    const name = newName.trim();
    if (
      !name ||
      duplicateName ||
      createMutation.isPending ||
      captureApps.length === 0
    )
      return;
    createMutation.mutate(
      {
        name,
        scope: scope ?? "claude",
        apps: captureApps,
      },
      { onSuccess: closeCreateDialog },
    );
  };

  return (
    <>
      <Popover
        open={open}
        onOpenChange={(nextOpen) => {
          setOpen(nextOpen);
          if (nextOpen) {
            void queryClient.invalidateQueries({ queryKey: ["profiles"] });
          }
        }}
      >
        <PopoverTrigger asChild>
          <button
            ref={triggerRef}
            type="button"
            role="combobox"
            aria-expanded={open}
            aria-label={t("profiles.switcherAriaLabel", {
              name: currentProfile?.name ?? t("profiles.none"),
            })}
            title={t("profiles.bundle.switcherHint")}
            className={cn(
              "inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-sm font-medium transition-colors",
              "hover:bg-[var(--mp-hover)]",
              currentProfile ? "text-foreground" : "text-muted-foreground",
              triggerClassName,
            )}
          >
            <FolderOpen className="h-4 w-4 shrink-0 opacity-70" />
            <span className="max-w-[9rem] truncate">
              {currentProfile?.name ??
                t(compact ? "trayPanel.schemes" : "profiles.none")}
            </span>
            <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          onCloseAutoFocus={(event) => {
            if (openingCreateDialog.current) {
              event.preventDefault();
              openingCreateDialog.current = false;
            }
          }}
          side={side}
          align="start"
          sideOffset={compact ? 4 : 6}
          collisionPadding={compact ? 8 : undefined}
          aria-label={t("profiles.bundle.switcherHint")}
          className={cn("z-[100] w-64 p-0", compact && "tray-profile-menu")}
        >
          <Command label={t("profiles.searchPlaceholder")}>
            <CommandInput placeholder={t("profiles.searchPlaceholder")} />
            <CommandList>
              <CommandEmpty>{t("profiles.empty")}</CommandEmpty>
              {profiles.length > 0 && (
                <CommandGroup>
                  {profiles.map((profile) => (
                    <CommandItem
                      key={profile.id}
                      value={profile.id}
                      keywords={[profile.name]}
                      onSelect={() => handleApply(profile.id)}
                    >
                      <Check
                        className={cn(
                          "mr-2 h-4 w-4 shrink-0",
                          currentId === profile.id
                            ? "opacity-100"
                            : "opacity-0",
                        )}
                      />
                      <span className="truncate">{profile.name}</span>
                      {!profile.payload.bundle &&
                        scope &&
                        !hasScopeSnapshot(profile, scope) && (
                          <span className="ml-auto shrink-0 pl-2 text-xs text-muted-foreground">
                            {t("profiles.noSnapshotForScope")}
                          </span>
                        )}
                    </CommandItem>
                  ))}
                </CommandGroup>
              )}
              {profiles.length > 0 && (
                <div className="mx-1 my-1 h-px bg-border" />
              )}
              <CommandGroup>
                <CommandItem
                  value="__create__"
                  keywords={[t("profiles.createFromCurrent")]}
                  onSelect={() => {
                    openingCreateDialog.current = true;
                    createMutation.reset();
                    setNewName("");
                    setOpen(false);
                    setCreateBundle(false);
                    setBundleApps(selectedAvailableApps);
                    setCaptureApp(activeApp);
                    setIsCreateOpen(true);
                  }}
                >
                  <Plus className="mr-2 h-4 w-4 shrink-0" />
                  {t("profiles.createFromCurrent")}
                </CommandItem>
                {data?.undo && (
                  <CommandItem
                    value="__undo__"
                    keywords={[t("profiles.bundle.undoLast")]}
                    onSelect={() => {
                      setOpen(false);
                      setPendingUndo(data.undo!);
                    }}
                  >
                    {t("profiles.bundle.undoLast")}
                  </CommandItem>
                )}
                {currentId && (
                  <CommandItem
                    value="__clear__"
                    keywords={[t("profiles.none")]}
                    onSelect={() => {
                      setOpen(false);
                      clearMutation.mutate(
                        data?.currentBundleId ? "bundle" : activeApp,
                      );
                    }}
                  >
                    <X className="mr-2 h-4 w-4 shrink-0" />
                    {t("profiles.none")}
                  </CommandItem>
                )}
                {profiles.length > 0 && (
                  <CommandItem
                    value="__manage__"
                    keywords={[t("profiles.manage")]}
                    onSelect={() => {
                      setOpen(false);
                      setIsManageOpen(true);
                    }}
                  >
                    <FolderCog className="mr-2 h-4 w-4 shrink-0" />
                    {t("profiles.manage")}
                  </CommandItem>
                )}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      <Dialog
        open={isCreateOpen}
        onOpenChange={(open) => {
          if (!open) closeCreateDialog();
        }}
      >
        <DialogContent
          className="max-w-md"
          zIndex="alert"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            triggerRef.current?.focus();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("profiles.createFromCurrent")}</DialogTitle>
            <DialogDescription>
              {t(
                createBundle
                  ? "profiles.bundle.captureHint"
                  : "profiles.bundle.captureCurrentHint",
                { app: t(`apps.${captureApp}`) },
              )}
            </DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-4 pb-5">
            <div className="space-y-1.5">
              <Input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder={t("profiles.namePlaceholder")}
                aria-label={t("profiles.name")}
                aria-invalid={duplicateName || undefined}
                aria-describedby={duplicateName ? nameErrorId : undefined}
              />
              {duplicateName && (
                <p
                  id={nameErrorId}
                  role="alert"
                  className="text-xs text-destructive"
                >
                  {t("profiles.nameExists")}
                </p>
              )}
            </div>
            <div className="rounded-md border border-border">
              <label className="flex min-h-11 cursor-pointer items-center gap-2.5 rounded-t-md bg-muted/50 px-3 py-2.5 font-medium leading-5">
                <Checkbox
                  checked={createBundle}
                  onCheckedChange={(checked) => {
                    setCreateBundle(checked === true);
                    setBundleApps(selectedAvailableApps);
                  }}
                />
                <span>{t("profiles.bundle.create")}</span>
              </label>
              <div className="border-t border-border px-3 py-3">
                {createBundle ? (
                  <fieldset className="min-w-0">
                    <legend className="mb-1 font-medium leading-5">
                      {t("profiles.bundle.chooseAgents")}
                    </legend>
                    <div className="divide-y divide-border">
                      {agents.map(
                        ({
                          app,
                          available,
                          reason,
                          mcp,
                          skills,
                          instructions,
                        }) => (
                          <label
                            key={app}
                            className="flex items-start gap-2.5 py-2.5 last:pb-0"
                          >
                            <Checkbox
                              className="mt-0.5"
                              disabled={!available}
                              checked={bundleApps.includes(app)}
                              onCheckedChange={(checked) =>
                                setBundleApps((previous) =>
                                  checked
                                    ? [...previous, app]
                                    : previous.filter((value) => value !== app),
                                )
                              }
                            />
                            <span className="min-w-0 break-words leading-5">
                              <span
                                className={cn(
                                  "font-medium",
                                  !available && "text-muted-foreground",
                                )}
                              >
                                {t(`apps.${app}`)}
                              </span>
                              {(available || reason) && (
                                <span className="block text-xs text-muted-foreground">
                                  {available &&
                                    [
                                      t("profiles.bundle.modelsLabel"),
                                      mcp && "MCP",
                                      skills && "Skills",
                                      instructions &&
                                        t("profiles.bundle.instructionsLabel"),
                                    ]
                                      .filter(Boolean)
                                      .join(" · ")}
                                  {reason && ` ${t(reason)}`}
                                </span>
                              )}
                            </span>
                          </label>
                        ),
                      )}
                    </div>
                  </fieldset>
                ) : (
                  <div className="space-y-2">
                    <label className="flex flex-col gap-2 font-medium leading-5">
                      {t("profiles.bundle.singleAgent")}
                      {nativeAgentPicker ? (
                        <select
                          aria-label={t("profiles.bundle.singleAgent")}
                          className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                          value={captureApp}
                          onChange={(event) =>
                            setCaptureApp(event.target.value as AppId)
                          }
                        >
                          {agents.map(({ app, available }) => (
                            <option key={app} value={app} disabled={!available}>
                              {t(`apps.${app}`)}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <Select
                          value={captureApp}
                          onValueChange={(value) =>
                            setCaptureApp(value as AppId)
                          }
                        >
                          <SelectTrigger
                            aria-label={t("profiles.bundle.singleAgent")}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {agents.map(({ app, available }) => (
                              <SelectItem
                                key={app}
                                value={app}
                                disabled={!available}
                              >
                                {t(`apps.${app}`)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      )}
                    </label>
                    {currentAgent?.reason && (
                      <p className="text-xs text-muted-foreground">
                        {t(currentAgent.reason)}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </div>
            {createMutation.isError && (
              <p role="alert" className="text-xs text-destructive">
                {t("profiles.createFailed", {
                  detail: profileErrorMessage(createMutation.error, t),
                })}
              </p>
            )}
          </DialogBody>
          <DialogFooter className="gap-3 border-t border-border py-4">
            <Button variant="outline" onClick={closeCreateDialog}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={handleCreate}
              disabled={
                !newName.trim() ||
                duplicateName ||
                createMutation.isPending ||
                captureApps.length === 0
              }
            >
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        isOpen={!!pendingUndo}
        zIndex="top"
        pending={undoing}
        variant="info"
        title={t("profiles.bundle.undoLast")}
        message={t("profiles.bundle.undoLastHint", {
          name: pendingUndo?.name,
          apps: pendingUndo?.apps.map((app) => t(`apps.${app}`)).join(" · "),
        })}
        onConfirm={() => void undoLast()}
        onCancel={() => {
          if (!undoing) setPendingUndo(null);
        }}
      />
      <ProfileManageDialog
        selectedApps={selectedApps}
        isOpen={isManageOpen}
        onClose={() => setIsManageOpen(false)}
      />
      {bundleDialog.snapshot && (
        <ProfileBundleDialog
          open={!!bundlePreview}
          profile={bundleDialog.snapshot}
          onExited={bundleDialog.onExited}
          onClose={() => setBundlePreview(null)}
          onChanged={() => {
            void queryClient.invalidateQueries({ queryKey: ["profiles"] });
          }}
        />
      )}
      {legacyDialog.snapshot && scope && (
        <ProfileLegacyDialog
          open={!!legacyPreview}
          profile={legacyDialog.snapshot}
          onExited={legacyDialog.onExited}
          scope={scope}
          onClose={() => setLegacyPreview(null)}
        />
      )}
    </>
  );
}
