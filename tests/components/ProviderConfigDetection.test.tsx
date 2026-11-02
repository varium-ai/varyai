import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderConfigDetection } from "@/components/providers/forms/ProviderConfigDetection";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import {
  detectProviderProtocols,
  type ProtocolDetection,
} from "@/lib/api/provider-protocol-detection";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/api/model-fetch", () => ({ fetchModelsForConfig: vi.fn() }));
vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn(),
}));

const props = () => ({
  baseUrl: "https://fixture.example/v1",
  apiKey: "synthetic-key",
  model: "fixture",
  apiFormat: "gemini_native" as const,
  editingCredentials: false,
  autoSelectFormat: true,
  onProbeModelChange: vi.fn(),
  onApiFormatChange: vi.fn(),
});
const verified: ProtocolDetection[] = [
  {
    format: "openai_chat",
    text: { status: "verified" },
    stream: { status: "verified" },
  },
];

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
  vi.mocked(detectProviderProtocols).mockResolvedValue([]);
});
afterEach(() => {
  vi.useRealTimers();
});
const settle = async () => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(850);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(850);
  });
};

describe("provider configuration detection", () => {
  it("keeps the selected probe model without changing the configured model", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "configured-model", ownedBy: null },
      { id: "probe-model", ownedBy: null },
    ]);
    const p = {
      ...props(),
      model: "configured-model",
      autoSelectFormat: false,
      onProbeModelChange: undefined,
    };
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.selectModel" }),
    );
    fireEvent.click(screen.getByRole("option", { name: "probe-model" }));
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    expect(detectProviderProtocols).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: "probe-model" }),
    );
    view.rerender(
      <ProviderConfigDetection {...p} model="another-configured-model" />,
    );
    await act(async () => {});
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    expect(detectProviderProtocols).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: "probe-model" }),
    );
  });

  it("tries both catalog formats and does not rediscover after selecting a protocol", async () => {
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    expect(fetchModelsForConfig).toHaveBeenLastCalledWith(
      p.baseUrl,
      p.apiKey,
      undefined,
      undefined,
      undefined,
      { apiFormat: undefined },
    );
    view.rerender(<ProviderConfigDetection {...p} apiFormat="openai_chat" />);
    await settle();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(2);
    expect(fetchModelsForConfig).toHaveBeenLastCalledWith(
      p.baseUrl,
      p.apiKey,
      undefined,
      undefined,
      undefined,
      { apiFormat: undefined },
    );
  });

  it("keeps failed explicit checks safe and allows retrying", async () => {
    vi.mocked(detectProviderProtocols).mockRejectedValueOnce(
      new Error("synthetic-private-input"),
    );
    const p = props();
    render(<ProviderConfigDetection {...p} />);
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "providerDetection.failed",
    );
    expect(
      screen.queryByText(/synthetic-private-input/),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "providerDetection.check" }),
    ).toBeEnabled();
  });

  it("requires a verified stream and applies explicit checks to existing providers", async () => {
    const p = props();
    vi.mocked(detectProviderProtocols).mockResolvedValueOnce([
      { ...verified[0], stream: { status: "invalid_response" } },
    ]);
    const view = render(<ProviderConfigDetection {...p} />);
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "providerDetection.use" }),
    ).not.toBeInTheDocument();
    vi.mocked(detectProviderProtocols).mockResolvedValueOnce(verified);
    view.rerender(<ProviderConfigDetection {...p} autoSelectFormat={false} />);
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(p.onApiFormatChange).toHaveBeenCalledExactlyOnceWith("openai_chat");
  });

  it("explicit checks replace a working manual protocol with the first fully verified protocol", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValue([
      { ...verified[0], stream: { status: "invalid_response" } },
      { ...verified[0], format: "openai_responses" },
      { ...verified[0], format: "anthropic" },
    ]);
    const p = props();
    render(
      <ProviderConfigDetection
        {...p}
        apiFormat="anthropic"
        autoSelectFormat={false}
      />,
    );
    await settle();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(p.onApiFormatChange).toHaveBeenCalledExactlyOnceWith(
      "openai_responses",
    );
  });

  it("reports an invalid URL only after an explicit click without making requests", async () => {
    render(<ProviderConfigDetection {...props()} baseUrl="wrong-url" />);
    await settle();
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "providerDetection.status.invalid_config",
    );
    expect(detectProviderProtocols).not.toHaveBeenCalled();
  });

  it("waits until credential editing ends and keeps catalog failures safe", async () => {
    vi.mocked(fetchModelsForConfig).mockRejectedValue(
      new Error("synthetic auth failure"),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} editingCredentials />);
    await settle();
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
    view.rerender(<ProviderConfigDetection {...p} />);
    await settle();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/synthetic auth failure/),
    ).not.toBeInTheDocument();
    expect(detectProviderProtocols).toHaveBeenCalledTimes(1);
  });

  it("discards old model catalogs when credentials change", async () => {
    let resolve!: (models: { id: string; ownedBy: null }[]) => void;
    vi.mocked(fetchModelsForConfig).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    view.rerender(
      <ProviderConfigDetection
        {...p}
        apiKey="new-synthetic-key"
        editingCredentials
      />,
    );
    await act(async () => {
      resolve([{ id: "old-model", ownedBy: null }]);
    });
    expect(
      screen.queryByText("providerDetection.modelsFound"),
    ).not.toBeInTheDocument();
    expect(p.onProbeModelChange).not.toHaveBeenCalled();
  });

  it("automatically selects the first verified protocol even when the default also works", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValue([
      ...verified,
      { ...verified[0], format: "gemini_native" },
    ]);
    const p = props();
    render(<ProviderConfigDetection {...p} />);
    await settle();
    expect(detectProviderProtocols).toHaveBeenCalledTimes(1);
    expect(p.onApiFormatChange).toHaveBeenCalledWith("openai_chat");
  });

  it("uses a discovered model to detect without replacing the agent model", async () => {
    vi.mocked(fetchModelsForConfig)
      .mockRejectedValueOnce(new Error("synthetic catalog failure"))
      .mockResolvedValueOnce([{ id: "catalog-model", ownedBy: null }]);
    vi.mocked(detectProviderProtocols).mockResolvedValue(verified);
    const p = { ...props(), model: "" };
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    await settle();
    expect(detectProviderProtocols).toHaveBeenCalledWith(
      expect.objectContaining({ model: "catalog-model" }),
    );
    expect(p.onProbeModelChange).not.toHaveBeenCalled();
    expect(p.onApiFormatChange).toHaveBeenCalledWith("openai_chat");
    view.rerender(<ProviderConfigDetection {...p} apiFormat="openai_chat" />);
    await settle();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(2);
    expect(detectProviderProtocols).toHaveBeenCalledTimes(1);
  });

  it("tests an available catalog model when the agent's default is unavailable", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "available-model", ownedBy: null },
    ]);
    vi.mocked(detectProviderProtocols).mockResolvedValue(verified);
    const p = props();
    render(<ProviderConfigDetection {...p} />);
    await settle();
    expect(detectProviderProtocols).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ model: "available-model" }),
    );
    expect(p.onProbeModelChange).not.toHaveBeenCalled();
    expect(p.onApiFormatChange).toHaveBeenCalledWith("openai_chat");
  });

  it("does not probe without a model or overwrite an existing provider automatically", async () => {
    const p = props();
    const view = render(<ProviderConfigDetection {...p} model="" />);
    await settle();
    await settle();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
    view.rerender(<ProviderConfigDetection {...p} autoSelectFormat={false} />);
    await settle();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
  });

  it("skips protocols without a verified stream and keeps the choice when all fail", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValueOnce([
      { ...verified[0], stream: { status: "invalid_response" } },
      { ...verified[0], format: "openai_responses" },
    ]);
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    expect(p.onApiFormatChange).toHaveBeenCalledExactlyOnceWith(
      "openai_responses",
    );
    vi.mocked(detectProviderProtocols).mockResolvedValueOnce([
      { ...verified[0], text: { status: "authentication" }, stream: null },
    ]);
    view.rerender(
      <ProviderConfigDetection {...p} apiKey="another-synthetic-key" />,
    );
    await settle();
    expect(p.onApiFormatChange).toHaveBeenCalledTimes(1);
  });

  it("does not repeat probes after blur or a programmatic protocol update", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValue(verified);
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    view.rerender(
      <ProviderConfigDetection
        {...p}
        apiFormat="openai_chat"
        editingCredentials
      />,
    );
    view.rerender(<ProviderConfigDetection {...p} apiFormat="openai_chat" />);
    await settle();
    expect(detectProviderProtocols).toHaveBeenCalledTimes(1);
  });

  it("discards pending detection after credentials change", async () => {
    let resolve!: (value: ProtocolDetection[]) => void;
    vi.mocked(detectProviderProtocols).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    await settle();
    view.rerender(
      <ProviderConfigDetection
        {...p}
        apiKey="changed-synthetic-key"
        editingCredentials
      />,
    );
    await act(async () => {
      resolve(verified);
    });
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
    expect(
      screen.queryByText("providerDetection.verified"),
    ).not.toBeInTheDocument();
  });

  it("does not overwrite a manual protocol selection made during detection", async () => {
    let resolve!: (value: ProtocolDetection[]) => void;
    vi.mocked(detectProviderProtocols).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    view.rerender(
      <ProviderConfigDetection
        {...p}
        apiFormat="anthropic"
        autoSelectFormat={false}
      />,
    );
    await act(async () => {
      resolve(verified);
    });
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
  });

  it("invalidates pending detection when the model changes", async () => {
    let resolve!: (value: ProtocolDetection[]) => void;
    vi.mocked(detectProviderProtocols).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    view.rerender(<ProviderConfigDetection {...p} model="new-model" />);
    await act(async () => {
      resolve(verified);
    });
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
    expect(
      screen.queryByText("providerDetection.verified"),
    ).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)(
    "keeps the newer model result when the previous model request finishes late (%s)",
    async (outcome) => {
      let resolveOld!: (value: ProtocolDetection[]) => void;
      let rejectOld!: (reason: Error) => void;
      const p = props();
      vi.mocked(detectProviderProtocols)
        .mockReturnValueOnce(
          new Promise((resolve, reject) => {
            resolveOld = resolve;
            rejectOld = reject;
          }),
        )
        .mockResolvedValueOnce(verified);
      const view = render(
        <ProviderConfigDetection
          {...p}
          model="qwen-unavailable"
          autoSelectFormat={false}
        />,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
      view.rerender(
        <ProviderConfigDetection
          {...p}
          model="glm-working"
          autoSelectFormat={false}
        />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: "providerDetection.check" }),
        );
      });
      expect(detectProviderProtocols).toHaveBeenLastCalledWith(
        expect.objectContaining({ model: "glm-working" }),
      );
      expect(
        screen.getByText("providerDetection.verified"),
      ).toBeInTheDocument();
      await act(async () => {
        if (outcome === "resolve")
          resolveOld([
            { ...verified[0], text: { status: "unavailable" }, stream: null },
          ]);
        else rejectOld(new Error("synthetic old request failure"));
      });
      expect(
        screen.getByText("providerDetection.verified"),
      ).toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(p.onApiFormatChange).toHaveBeenCalledExactlyOnceWith(
        "openai_chat",
      );
    },
  );

  it("invalidates the old request on model selection before the parent updates its props", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    let resolveOld!: (value: ProtocolDetection[]) => void;
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "qwen-unavailable", ownedBy: null },
      { id: "glm-working", ownedBy: null },
    ]);
    vi.mocked(detectProviderProtocols).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    const p = props();
    const view = render(
      <ProviderConfigDetection
        {...p}
        model="qwen-unavailable"
        autoSelectFormat={false}
      />,
    );
    await settle();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.selectModel" }),
    );
    fireEvent.click(screen.getByRole("option", { name: "glm-working" }));
    expect(p.onProbeModelChange).toHaveBeenCalledWith("glm-working");
    await act(async () => {
      resolveOld(verified);
    });
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
    expect(
      screen.queryByText("providerDetection.verified"),
    ).not.toBeInTheDocument();
    view.rerender(
      <ProviderConfigDetection
        {...p}
        model="glm-working"
        autoSelectFormat={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: "providerDetection.check" }),
    ).toBeEnabled();
  });
});
