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
  onModelChange: vi.fn(),
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
});
afterEach(() => {
  vi.useRealTimers();
});
const settle = async () => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(850);
  });
};

describe("provider configuration detection", () => {
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

  it("requires a verified stream and preserves existing providers", async () => {
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
    expect(p.onApiFormatChange).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.use" }),
    );
    expect(p.onApiFormatChange).toHaveBeenCalledWith("openai_chat");
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

  it("waits until credential editing ends and keeps background failures silent", async () => {
    vi.mocked(fetchModelsForConfig).mockRejectedValue(
      new Error("synthetic auth failure"),
    );
    const p = props();
    const view = render(<ProviderConfigDetection {...p} editingCredentials />);
    await settle();
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
    view.rerender(<ProviderConfigDetection {...p} />);
    await settle();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/synthetic auth failure/),
    ).not.toBeInTheDocument();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
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
    expect(p.onModelChange).not.toHaveBeenCalled();
  });

  it("only probes on an explicit click and auto-selects a verified streaming protocol", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValue(verified);
    const p = props();
    render(<ProviderConfigDetection {...p} />);
    await settle();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(detectProviderProtocols).toHaveBeenCalledTimes(1);
    expect(p.onApiFormatChange).toHaveBeenCalledWith("openai_chat");
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
    view.rerender(<ProviderConfigDetection {...p} autoSelectFormat={false} />);
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
});
