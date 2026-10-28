import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProfileBundleDialog } from "@/components/profiles/ProfileBundleDialog";
import { profilesApi } from "@/lib/api/profiles";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { toast } from "sonner";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), warning: vi.fn() } }));

vi.mock("@/lib/api/profiles", () => ({
  profilesApi: {
    previewBundle: vi.fn(),
    applyBundle: vi.fn(),
    undoBundle: vi.fn(),
  },
}));

describe("cross-agent profile preview", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    i18n.addResourceBundle("zh", "translation", zh, true, true);
  });

  it("requires reviewing both agents before applying the revision shown", async () => {
    const onClose = vi.fn();
    vi.mocked(profilesApi.previewBundle).mockResolvedValue({
      revision: "reviewed-revision",
      changes: [
        {
          app: "claude",
          currentProvider: "Old Claude",
          targetProvider: "Work Claude",
          models: ["model-a"],
        },
        {
          app: "codex",
          currentProvider: "Old Codex",
          targetProvider: "Work Codex",
          models: ["model-b"],
        },
      ],
    });
    vi.mocked(profilesApi.applyBundle).mockResolvedValue({
      success: true,
      undoId: "undo-1",
      warnings: [],
    });
    render(
      <ProfileBundleDialog
        profile={{ id: "work", name: "Work" }}
        onClose={onClose}
        onChanged={vi.fn()}
      />,
    );
    await screen.findByText("Work Claude");
    expect(screen.getByText("Work Codex")).toBeInTheDocument();
    expect(
      screen.getByText(/单供应商切换只影响当前 Agent/),
    ).toBeInTheDocument();
    expect(screen.getByText(/任一工具的本地配置应用失败/)).toBeInTheDocument();
    expect(
      screen.getByText("Claude：重新启动 CLI 或新建会话后生效。"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Codex：请完全退出并重新打开客户端；CLI 请重新启动会话。",
      ),
    ).toBeInTheDocument();
    expect(profilesApi.applyBundle).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "应用方案" }));
    await waitFor(() =>
      expect(profilesApi.applyBundle).toHaveBeenCalledWith(
        "work",
        "reviewed-revision",
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(
      screen.queryByRole("button", { name: "恢复切换前配置" }),
    ).not.toBeInTheDocument();
    expect(toast.success).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        description: expect.stringContaining(
          "Codex：请完全退出并重新打开客户端",
        ),
      }),
    );
  });

  it.each([false, true])(
    "shows provider and model changes without resource details (saved resources: %s)",
    async (hasResources) => {
      vi.mocked(profilesApi.previewBundle).mockResolvedValue({
        revision: "resources",
        changes: [
          {
            app: "gemini",
            currentProvider: "Before",
            targetProvider: "After",
            currentModels: ["old-model"],
            models: ["new-model"],
            enabledProviders: ["After"],
            currentLibrary: {
              mcp: [],
              skills: [],
              prompts: [],
              instructions: false,
              templates: [],
            },
            library: {
              mcp: hasResources ? ["Filesystem"] : [],
              skills: hasResources ? ["Review"] : [],
              prompts: hasResources ? ["Work"] : [],
              instructions: hasResources,
              templates: hasResources ? ["Template"] : [],
            },
          },
        ],
      });
      render(
        <ProfileBundleDialog
          profile={{ id: "full", name: "Full" }}
          onClose={vi.fn()}
          onChanged={vi.fn()}
        />,
      );
      expect(
        await screen.findByText(/old-model.*new-model/),
      ).toBeInTheDocument();
      expect(screen.getByText("After")).toBeInTheDocument();
      expect(
        screen.queryAllByText(/^(MCP|Skills|提示词|指令文件|提示词模板):/),
      ).toHaveLength(0);
      for (const name of ["Filesystem", "Review", "Work", "Template"]) {
        expect(screen.queryByText(new RegExp(name))).not.toBeInTheDocument();
      }
      expect(
        screen.getByText(/核对以下工具的供应商.*模型/),
      ).not.toHaveTextContent("资源");
    },
  );

  it("shows a failed preview and never allows an unreviewed apply", async () => {
    vi.mocked(profilesApi.previewBundle).mockRejectedValue(
      new Error("profiles.bundle.conflict"),
    );
    render(
      <ProfileBundleDialog
        profile={{ id: "work", name: "Work" }}
        onClose={vi.fn()}
        onChanged={vi.fn()}
      />,
    );
    await screen.findByRole("alert");
    expect(
      screen.queryByRole("button", { name: "应用方案" }),
    ).not.toBeInTheDocument();
    expect(profilesApi.applyBundle).not.toHaveBeenCalled();
  });

  it("allows a pending client to apply local settings and explains the separate sync", async () => {
    const onClose = vi.fn();
    vi.mocked(profilesApi.previewBundle).mockResolvedValue({
      revision: "offline",
      changes: [
        {
          app: "trae",
          currentProvider: "Before",
          targetProvider: "After",
          models: ["model-a"],
          clientSyncPending: true,
        },
      ],
    });
    vi.mocked(profilesApi.applyBundle).mockResolvedValue({
      success: true,
      undoId: "offline-undo",
      warnings: ["profiles.bundle.clientSyncPending"],
    });
    render(
      <ProfileBundleDialog
        profile={{ id: "offline", name: "Offline" }}
        onClose={onClose}
        onChanged={vi.fn()}
      />,
    );
    await screen.findByText("After");
    expect(screen.getByText(/TRAE：方案可离线切换/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "应用方案" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(toast.warning).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        description: expect.stringContaining("连接后将自动生效"),
      }),
    );
    expect(profilesApi.applyBundle).toHaveBeenCalledWith("offline", "offline");
  });

  it("keeps the preview open when local apply fails and rolls back", async () => {
    const onClose = vi.fn();
    vi.mocked(profilesApi.previewBundle).mockResolvedValue({
      revision: "failed",
      changes: [],
    });
    vi.mocked(profilesApi.applyBundle).mockResolvedValue({
      success: false,
      undoId: null,
      warnings: ["profiles.bundle.applyRolledBack"],
    });
    render(
      <ProfileBundleDialog
        profile={{ id: "work", name: "Work" }}
        onClose={onClose}
        onChanged={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "应用方案" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "应用失败，已恢复切换前配置",
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("identifies the failed agent and missing native model after rollback", async () => {
    vi.mocked(profilesApi.previewBundle).mockResolvedValue({
      revision: "failed",
      changes: [],
    });
    vi.mocked(profilesApi.applyBundle).mockResolvedValue({
      success: false,
      undoId: null,
      warnings: [],
      failure: { app: "qwen", reason: "profiles.bundle.nativeModelMissing" },
    } as Awaited<ReturnType<typeof profilesApi.applyBundle>>);
    render(
      <ProfileBundleDialog
        profile={{ id: "work", name: "Work" }}
        onClose={vi.fn()}
        onChanged={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "应用方案" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Qwen Code");
    expect(alert).toHaveTextContent("模型或协议/地址与当前配置不匹配");
    expect(alert).toHaveTextContent("已恢复切换前配置");
  });

  it.each(Object.entries(zh.apps))(
    "identifies %s failures without exposing raw errors",
    async (app, label) => {
      vi.mocked(profilesApi.previewBundle).mockResolvedValue({
        revision: "failed",
        changes: [],
      });
      vi.mocked(profilesApi.applyBundle).mockResolvedValue({
        success: false,
        undoId: null,
        warnings: [],
        failure: {
          app,
          reason: "synthetic-private-key: upstream request failed",
        },
      });
      render(
        <ProfileBundleDialog
          profile={{ id: "work", name: "Work" }}
          onClose={vi.fn()}
          onChanged={vi.fn()}
        />,
      );
      fireEvent.click(await screen.findByRole("button", { name: "应用方案" }));
      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent(label);
      expect(alert).toHaveTextContent("操作失败，请检查配置并重试");
      expect(alert).toHaveTextContent("已恢复切换前配置");
      expect(alert).not.toHaveTextContent("synthetic-private-key");
    },
  );
});
