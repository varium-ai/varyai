import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QoderCertificatePanel } from "@/components/QoderCertificatePanel";

const state = vi.hoisted(() => ({
  data: {
    expires_at: 1800000000 as number | null,
    expired: false,
    fingerprint: "ABC123",
    active: true,
  },
  invoke: vi.fn(),
  invalidate: vi.fn(),
  error: vi.fn(),
  success: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: state.invoke }));
vi.mock("@tanstack/react-query", () => ({
  useQuery: () => ({ data: state.data }),
  useQueryClient: () => ({ invalidateQueries: state.invalidate }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "en" },
    t: (key: string, values?: any) =>
      values?.date ? `${key}: ${values.date}` : key,
  }),
}));
vi.mock("sonner", () => ({
  toast: { success: state.success, error: state.error },
}));
describe("Qoder certificate management", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.invoke.mockResolvedValue(undefined);
    state.data = {
      expires_at: 1800000000,
      expired: false,
      fingerprint: "ABC123",
      active: true,
    };
  });
  it("starts collapsed with expiry visible and toggles the certificate details", () => {
    render(<QoderCertificatePanel />);
    const toggle = screen.getByRole("button", {
      name: "qoder.certificate.manageTitle",
    });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText(/qoder.certificate.expiresAt:/)).toBeVisible();
    expect(screen.getByText("qoder.certificate.purpose")).not.toBeVisible();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("qoder.certificate.purpose")).toBeVisible();
    expect(screen.getByText("ABC123")).toBeVisible();
    fireEvent.click(toggle);
    expect(screen.getByText("qoder.certificate.purpose")).not.toBeVisible();
  });
  it("requires explicit confirmation before removing trust and refreshes the route status", async () => {
    render(<QoderCertificatePanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.manageTitle" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.clear" }),
    );
    expect(state.invoke).not.toHaveBeenCalled();
    expect(
      screen.getByText("qoder.certificate.clearDescription"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    await waitFor(() =>
      expect(state.invoke).toHaveBeenCalledWith("clear_qoder_certificate"),
    );
    await waitFor(() => expect(state.invalidate).toHaveBeenCalled());
  });
  it("can rebuild a still-valid certificate, rather than silently reusing it", async () => {
    render(<QoderCertificatePanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.manageTitle" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.rebuild" }),
    );
    expect(state.invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    await waitFor(() =>
      expect(state.invoke).toHaveBeenCalledWith("rebuild_qoder_certificate"),
    );
  });
  it("does not offer clearing an absent certificate", () => {
    state.data.expires_at = null;
    render(<QoderCertificatePanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "qoder.certificate.manageTitle" }),
    );
    expect(screen.getByText("qoder.certificate.absent")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "qoder.certificate.clear" }),
    ).toBeDisabled();
  });
});
