import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SecurityAuditToggle } from "@/components/security/SecurityAuditToggle";

const state = vi.hoisted(() => ({
  settings: {
    enabled: true,
  },
  connected: true,
  supported: true,
  isLoading: false,
  isUpdating: false,
  setEnabled: vi.fn(),
}));
vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => state,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("SecurityAuditToggle", () => {
  it("exposes concrete protection scope when connected", async () => {
    render(<SecurityAuditToggle appType="codex" />);
    fireEvent.pointerMove(screen.getByTestId("security-audit-toggle"));
    expect(
      await screen.findByRole("tooltip", {
        name: "homeAudit.protectionDetail",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveClass(
      "data-[state=checked]:bg-primary",
    );
    expect(screen.queryByText("homeAudit.enabled")).not.toBeInTheDocument();
    expect(
      screen.getByRole("switch").parentElement?.querySelector("svg"),
    ).toBeNull();
  });
  beforeEach(() => {
    state.supported = true;
    state.settings.enabled = true;
    state.connected = true;
    state.isLoading = false;
    state.isUpdating = false;
    state.setEnabled.mockReset().mockResolvedValue(undefined);
  });
  it("describes routing coverage for existing Pi providers", async () => {
    render(<SecurityAuditToggle appType="pi" />);
    fireEvent.pointerMove(screen.getByTestId("security-audit-toggle"));
    expect(
      await screen.findByRole("tooltip", { name: "piRouting.description" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("homeAudit.protectionDetail"),
    ).not.toBeInTheDocument();
  });
  it("changes only enabled while preserving audit policy", async () => {
    render(<SecurityAuditToggle appType="codex" />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(state.setEnabled).toHaveBeenCalledWith(false));
  });
  it("does not claim protection when the app is disconnected", async () => {
    state.connected = false;
    render(<SecurityAuditToggle appType="codex" />);
    fireEvent.pointerMove(screen.getByTestId("security-audit-toggle"));
    expect(
      await screen.findByRole("tooltip", {
        name: "homeAudit.disconnectedDetail",
      }),
    ).toBeInTheDocument();
  });
  it("disables changes while loading or saving", () => {
    state.isUpdating = true;
    render(<SecurityAuditToggle appType="codex" />);
    expect(screen.getByRole("switch")).toBeDisabled();
  });
  it("can enable global protection while viewing an unsupported client", async () => {
    state.supported = false;
    state.settings.enabled = false;
    render(<SecurityAuditToggle appType="opencode" />);
    expect(screen.getByRole("switch")).toBeEnabled();
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(state.setEnabled).toHaveBeenCalledWith(true));
  });
  it("keeps the persisted state when update fails", async () => {
    state.setEnabled.mockRejectedValue(new Error("offline"));
    render(<SecurityAuditToggle appType="codex" />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(state.setEnabled).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });
});
