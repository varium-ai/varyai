import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QoderCertificateNotice } from "@/components/QoderCertificateNotice";

const state = vi.hoisted(() => ({
  data: { expires_at: 100 as number | null, expired: false },
  invalidateQueries: vi.fn(),
  renew: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { setProxyTakeoverForApp: state.renew },
}));
vi.mock("sonner", () => ({
  toast: { success: state.success, error: state.error },
}));
vi.mock("@tanstack/react-query", () => ({
  useQuery: () => ({ data: state.data }),
  useQueryClient: () => ({ invalidateQueries: state.invalidateQueries }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("Qoder certificate expiry notice", () => {
  beforeEach(() => {
    state.data = { expires_at: 100, expired: false };
    state.renew.mockReset().mockResolvedValue(undefined);
    state.success.mockReset();
    state.error.mockReset();
  });
  it("renews from the expiry dialog without restarting VaryAI, then refreshes protection", async () => {
    state.data = { expires_at: 100, expired: true };
    render(<QoderCertificateNotice />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.renew" }),
    );
    await waitFor(() =>
      expect(state.success).toHaveBeenCalledWith(
        "qoder.certificate.renewed",
        expect.anything(),
      ),
    );
    expect(state.renew).toHaveBeenCalledWith("qoder", true);
    expect(state.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["qoderCertificateStatus"],
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("keeps the notice retryable when system authentication fails", async () => {
    state.data = { expires_at: 100, expired: true };
    state.renew.mockRejectedValueOnce(new Error("private-auth-diagnostic"));
    render(<QoderCertificateNotice />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.renew" }),
    );
    await waitFor(() =>
      expect(state.error).toHaveBeenCalledWith("qoder.certificate.renewFailed"),
    );
    expect(
      screen.getByRole("button", { name: "qoder.certificate.renew" }),
    ).toBeEnabled();
    expect(
      screen.queryByText("private-auth-diagnostic"),
    ).not.toBeInTheDocument();
  });
  it("keeps progress and retry visible while the backend temporarily disables routing", async () => {
    state.data = { expires_at: 100, expired: true };
    let reject!: (reason: Error) => void;
    state.renew.mockReturnValueOnce(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    const view = render(<QoderCertificateNotice />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.renew" }),
    );
    state.data = { expires_at: null, expired: false };
    view.rerender(<QoderCertificateNotice />);
    expect(
      screen.getByRole("button", { name: "qoder.certificate.renewing" }),
    ).toBeDisabled();
    await act(async () => reject(new Error("authentication timeout")));
    expect(
      screen.getByRole("button", { name: "qoder.certificate.renew" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("warns once per expired certificate, regardless of selected app, and refreshes protection status", () => {
    const view = render(<QoderCertificateNotice />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    state.data = { expires_at: 100, expired: true };
    view.rerender(<QoderCertificateNotice />);
    expect(
      screen.getByText("qoder.certificate.expiredTitle"),
    ).toBeInTheDocument();
    expect(state.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["proxyTakeoverStatus"],
    });
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    view.rerender(<QoderCertificateNotice />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    state.data = { expires_at: 200, expired: false };
    view.rerender(<QoderCertificateNotice />);
    state.data = { expires_at: 200, expired: true };
    view.rerender(<QoderCertificateNotice />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
