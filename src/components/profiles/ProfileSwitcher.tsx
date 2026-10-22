import { useState } from "react";
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
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api/types";
import {
  useApplyProfileMutation,
  useClearProfileMutation,
  useCreateProfileMutation,
  useProfilesQuery,
} from "@/lib/query/profiles";
import { ProfileManageDialog } from "./ProfileManageDialog";
import { ProfileBundleDialog } from "./ProfileBundleDialog";
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

const CURRENT_ID_KEY: Record<ProfileScope, keyof CurrentProfileIds> = {
  claude: "claude",
  "claude-desktop": "claudeDesktop",
  codex: "codex",
};

interface ProfileSwitcherProps {
  activeApp: AppId;
}

/** Shared project menu: legacy profiles apply to their scope; bundles preview all selected agents. */
export function ProfileSwitcher({ activeApp }: ProfileSwitcherProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isManageOpen, setIsManageOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [bundleApps, setBundleApps] = useState<AppId[]>([]);
  const [createBundle, setCreateBundle] = useState(false);
  const [pendingUndo, setPendingUndo] = useState<ProfileUndo | null>(null);
  const [undoing, setUndoing] = useState(false);
  const [bundlePreview, setBundlePreview] = useState<Profile | null>(null);
  const queryClient = useQueryClient();

  const { data } = useProfilesQuery();
  const applyMutation = useApplyProfileMutation();
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
  const isBundle = createBundle || !scope;
  const currentId =
    data?.currentIds?.byApp?.[activeApp] ??
    (scope ? data?.currentIds?.[CURRENT_ID_KEY[scope]] : null) ??
    null;
  const currentProfile = profiles.find((p) => p.id === currentId);

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
    if (scope && id !== currentId) {
      applyMutation.mutate({ id, scope });
    }
  };

  const closeCreateDialog = () => {
    setIsCreateOpen(false);
    setNewName("");
    setBundleApps([]);
    setCreateBundle(false);
  };

  const handleCreate = () => {
    const name = newName.trim();
    if (!name || (isBundle && bundleApps.length < 2)) return;
    createMutation.mutate(
      {
        name,
        scope: scope ?? "claude",
        apps: isBundle ? bundleApps : undefined,
      },
      { onSuccess: closeCreateDialog },
    );
  };

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            aria-expanded={open}
            aria-label={t("profiles.switcherAriaLabel", {
              name: currentProfile?.name ?? t("profiles.none"),
            })}
            title={t("profiles.bundle.switcherHint")}
            className={cn(
              "inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-sm font-medium transition-colors",
              "hover:bg-black/5 dark:hover:bg-white/5",
              currentProfile ? "text-foreground" : "text-muted-foreground",
            )}
          >
            <FolderOpen className="h-4 w-4 shrink-0 opacity-70" />
            <span className="max-w-[9rem] truncate">
              {currentProfile?.name ?? t("profiles.none")}
            </span>
            <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="bottom"
          align="start"
          sideOffset={6}
          aria-label={t("profiles.bundle.switcherHint")}
          className="z-[100] w-64 p-0"
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
              <div className="mx-1 my-1 h-px bg-border" />
              <CommandGroup>
                <CommandItem
                  value="__create__"
                  keywords={[t("profiles.createFromCurrent")]}
                  onSelect={() => {
                    setOpen(false);
                    setBundleApps(
                      agents.filter((a) => a.available).map((a) => a.app),
                    );
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
                      clearMutation.mutate(activeApp);
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
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle>{t("profiles.createFromCurrent")}</DialogTitle>
            <DialogDescription>
              {t(
                isBundle
                  ? "profiles.bundle.captureHint"
                  : `profiles.createDescription.${scope}`,
              )}
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 pt-3">
            <Input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t("profiles.namePlaceholder")}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreate();
              }}
            />
            <label className="mt-3 flex items-center gap-2 text-sm">
              <Checkbox
                checked={isBundle}
                disabled={!scope}
                onCheckedChange={(checked) => {
                  setCreateBundle(checked === true);
                  setBundleApps(
                    agents.filter((a) => a.available).map((a) => a.app),
                  );
                }}
              />
              {t("profiles.bundle.create")}
            </label>
            {isBundle && (
              <fieldset className="mt-3 max-h-[45vh] space-y-3 overflow-y-auto text-sm">
                <legend className="mb-2 text-muted-foreground">
                  {t("profiles.bundle.chooseAgents")}
                </legend>
                {agents.map(
                  ({ app, available, reason, mcp, skills, instructions }) => (
                    <label key={app} className="flex items-start gap-2">
                      <Checkbox
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
                      <span>
                        <span
                          className={!available ? "text-muted-foreground" : ""}
                        >
                          {t(`apps.${app}`)}
                        </span>
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
                      </span>
                    </label>
                  ),
                )}
                <p className="text-muted-foreground">
                  {t("profiles.bundle.captureHint")}
                </p>
              </fieldset>
            )}
          </div>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeCreateDialog}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={handleCreate}
              disabled={
                !newName.trim() ||
                createMutation.isPending ||
                (isBundle && bundleApps.length < 2)
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
        isOpen={isManageOpen}
        onClose={() => setIsManageOpen(false)}
      />
      {bundlePreview && (
        <ProfileBundleDialog
          profile={bundlePreview}
          onClose={() => setBundlePreview(null)}
          onChanged={() => {
            void queryClient.invalidateQueries({ queryKey: ["profiles"] });
          }}
        />
      )}
    </>
  );
}
