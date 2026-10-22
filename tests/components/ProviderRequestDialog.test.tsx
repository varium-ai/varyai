import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderRequestDialog } from "@/components/providers/forms/ProviderRequestDialog";
import { providerRequestsApi } from "@/lib/api/providerRequests";

vi.mock("@/lib/api/providerRequests", () => ({
  providerRequestsApi: { submit: vi.fn() },
}));

function renderDialog() {
  const onOpenChange = vi.fn();
  const onParentSubmit = vi.fn();
  render(
    <form onSubmit={onParentSubmit}>
      <ProviderRequestDialog open onOpenChange={onOpenChange} />
    </form>,
  );
  return { onOpenChange, onParentSubmit };
}

async function fillRequest() {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("供应商名称"), " Example AI ");
  await user.type(screen.getByLabelText("供应商官网"), "https://example.com");
  await user.type(
    screen.getByLabelText("补充说明（选填）"),
    "希望添加编码套餐",
  );
  return user;
}

describe("ProviderRequestDialog", () => {
  beforeEach(() => {
    vi.mocked(providerRequestsApi.submit).mockReset();
  });

  it("空输入框显示填写示例，示例不会作为默认值提交", () => {
    renderDialog();
    for (const placeholder of [
      "例如：Example AI",
      "https://example.com",
      "例如：希望支持该供应商的编码套餐",
    ]) {
      expect(screen.getByPlaceholderText(placeholder)).toHaveValue("");
    }
    fireEvent.submit(
      screen.getByRole("button", { name: "提交申请" }).closest("form")!,
    );
    expect(providerRequestsApi.submit).not.toHaveBeenCalled();
  });

  it("校验必填字段，不发送空申请", () => {
    renderDialog();
    fireEvent.submit(
      screen.getByRole("button", { name: "提交申请" }).closest("form")!,
    );
    expect(providerRequestsApi.submit).not.toHaveBeenCalled();
    expect(screen.getByText("请输入供应商名称")).toBeInTheDocument();
    expect(screen.getByText("请输入供应商官网")).toBeInTheDocument();
    expect(screen.getByLabelText("供应商名称")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(screen.getByLabelText("供应商官网")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("名称最多 50 字符，超出时阻止提交并在输入框下提示", () => {
    renderDialog();
    fireEvent.change(screen.getByLabelText("供应商名称"), {
      target: { value: "中".repeat(51) },
    });
    fireEvent.change(screen.getByLabelText("供应商官网"), {
      target: { value: "https://example.com" },
    });
    fireEvent.submit(
      screen.getByRole("button", { name: "提交申请" }).closest("form")!,
    );
    expect(providerRequestsApi.submit).not.toHaveBeenCalled();
    expect(
      screen.getByText("供应商名称不能超过 50 个字符"),
    ).toBeInTheDocument();
  });

  it.each([
    "",
    "  ",
    "example.com",
    "ftp://example.com",
    "https:example.com",
    "https:///example.com",
    "https://",
    "https://example.com:99999",
    "https://example.com/path with space",
    "https://user:secret@example.com",
    "https://@example.com",
    "https://example.com/\\path",
  ])("拒绝无效官网 %j，仅提交时显示错误且不发出申请", (website) => {
    renderDialog();
    fireEvent.change(screen.getByLabelText("供应商名称"), {
      target: { value: "Example AI" },
    });
    const input = screen.getByLabelText("供应商官网");
    fireEvent.change(input, { target: { value: website } });
    fireEvent.blur(input);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(input).toHaveAttribute("aria-invalid", "false");
    fireEvent.submit(
      screen.getByRole("button", { name: "提交申请" }).closest("form")!,
    );
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("alert")).toHaveTextContent(
      website.trim()
        ? "请输入有效的 http:// 或 https:// 官网链接"
        : "请输入供应商官网",
    );
    fireEvent.submit(
      screen.getByRole("button", { name: "提交申请" }).closest("form")!,
    );
    expect(providerRequestsApi.submit).not.toHaveBeenCalled();
  });

  it.each(["http://example.com", "https://example.com/path?a=1#details"])(
    "允许 50 字符名称与合法官网 %s，修正输入后清除错误",
    async (website) => {
      vi.mocked(providerRequestsApi.submit).mockResolvedValue(undefined);
      const { onOpenChange } = renderDialog();
      const name = screen.getByLabelText("供应商名称");
      const url = screen.getByLabelText("供应商官网");
      fireEvent.blur(name);
      fireEvent.blur(url);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      fireEvent.submit(
        screen.getByRole("button", { name: "提交申请" }).closest("form")!,
      );
      expect(screen.getAllByRole("alert")).toHaveLength(2);
      fireEvent.change(name, { target: { value: "中".repeat(50) } });
      fireEvent.change(url, { target: { value: website } });
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      fireEvent.submit(
        screen.getByRole("button", { name: "提交申请" }).closest("form")!,
      );
      await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
      expect(providerRequestsApi.submit).toHaveBeenCalledWith(
        expect.objectContaining({ name: "中".repeat(50), websiteUrl: website }),
      );
    },
  );

  it("修改已报错字段后清除提示，失焦不重新校验，下次提交才显示错误", () => {
    renderDialog();
    const form = screen
      .getByRole("button", { name: "提交申请" })
      .closest("form")!;
    fireEvent.submit(form);
    expect(screen.getAllByRole("alert")).toHaveLength(2);
    const name = screen.getByLabelText("供应商名称");
    const url = screen.getByLabelText("供应商官网");
    fireEvent.change(name, { target: { value: "中".repeat(51) } });
    fireEvent.change(url, { target: { value: "invalid-url" } });
    fireEvent.blur(name);
    fireEvent.blur(url);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.submit(form);
    expect(
      screen.getByText("供应商名称不能超过 50 个字符"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("请输入有效的 http:// 或 https:// 官网链接"),
    ).toBeInTheDocument();
    expect(providerRequestsApi.submit).not.toHaveBeenCalled();
  });

  it("提交填写的申请且不触发外层供应商保存，仅确认成功后关闭", async () => {
    vi.mocked(providerRequestsApi.submit).mockResolvedValue(undefined);
    const { onOpenChange, onParentSubmit } = renderDialog();
    const user = await fillRequest();
    await user.click(screen.getByRole("button", { name: "提交申请" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(providerRequestsApi.submit).toHaveBeenCalledWith({
      requestId: expect.stringMatching(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      ),
      name: "Example AI",
      websiteUrl: "https://example.com",
      description: "希望添加编码套餐",
    });
    expect(onParentSubmit).not.toHaveBeenCalled();
  });

  it("服务暂不可用时保留输入且不显示成功", async () => {
    vi.mocked(providerRequestsApi.submit).mockRejectedValue({
      code: "temporarily_unavailable",
    });
    const { onOpenChange } = renderDialog();
    const user = await fillRequest();
    await user.click(screen.getByRole("button", { name: "提交申请" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "服务暂不可用，请稍后重试",
    );
    expect(screen.getByLabelText("供应商名称")).toHaveValue(" Example AI ");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("WebView 缺少原生 randomUUID 时仍可生成 UUID v4 并提交", async () => {
    const getRandomValues = globalThis.crypto.getRandomValues.bind(
      globalThis.crypto,
    );
    vi.stubGlobal("crypto", { getRandomValues });
    try {
      vi.mocked(providerRequestsApi.submit).mockResolvedValue(undefined);
      const { onOpenChange } = renderDialog();
      const user = await fillRequest();
      await user.click(screen.getByRole("button", { name: "提交申请" }));
      await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
      expect(
        vi.mocked(providerRequestsApi.submit).mock.calls[0][0].requestId,
      ).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("失败重试复用申请 ID，修改内容后生成新 ID", async () => {
    vi.mocked(providerRequestsApi.submit).mockRejectedValue({
      code: "send_failed",
    });
    renderDialog();
    const user = await fillRequest();
    const submit = screen.getByRole("button", { name: "提交申请" });
    await user.click(submit);
    await screen.findByRole("alert");
    await user.click(submit);
    await waitFor(() =>
      expect(providerRequestsApi.submit).toHaveBeenCalledTimes(2),
    );
    const [first, second] = vi.mocked(providerRequestsApi.submit).mock.calls;
    expect(first[0]).toHaveProperty("requestId");
    expect(second[0]).toEqual(first[0]);
    await user.type(screen.getByLabelText("补充说明（选填）"), "，增加说明");
    await user.click(submit);
    const third = vi.mocked(providerRequestsApi.submit).mock.calls[2][0];
    expect(third.requestId).not.toEqual(first[0].requestId);
  });

  it("限流展示服务端等待秒数，不回显响应正文", async () => {
    vi.mocked(providerRequestsApi.submit).mockRejectedValue({
      code: "rate_limited",
      retryAfterSeconds: 60,
      message: "private-response",
    });
    const { onOpenChange } = renderDialog();
    const user = await fillRequest();
    await user.click(screen.getByRole("button", { name: "提交申请" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "提交过于频繁，请在 60 秒后重试",
    );
    expect(screen.queryByText(/private-response/)).not.toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("发送期间阻止重复提交和关闭，错误不会回显接口详情", async () => {
    let rejectRequest!: (reason: unknown) => void;
    vi.mocked(providerRequestsApi.submit).mockImplementation(
      () =>
        new Promise((_, reject) => {
          rejectRequest = reject;
        }),
    );
    const { onOpenChange } = renderDialog();
    const user = await fillRequest();
    await user.click(screen.getByRole("button", { name: "提交申请" }));
    const loadingButton = screen.getByRole("button", { name: "提交中…" });
    expect(loadingButton).toBeDisabled();
    expect(loadingButton).toHaveAttribute("aria-busy", "true");
    expect(loadingButton.querySelector("svg")).toHaveClass("animate-spin");
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
    rejectRequest(new Error("secret-webhook-response"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "发送失败，请检查网络后重试",
    );
    expect(screen.queryByText(/secret-webhook/)).not.toBeInTheDocument();
    expect(providerRequestsApi.submit).toHaveBeenCalledOnce();
  });
});
