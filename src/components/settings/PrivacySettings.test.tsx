import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PrivacySettings } from "./PrivacySettings";

const api = vi.hoisted(() => ({
  getPreferences: vi.fn(),
  setEnabled: vi.fn(),
}));
vi.mock("@/lib/api/telemetry", () => ({ telemetryApi: api }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function show() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <PrivacySettings />
    </QueryClientProvider>,
  );
}

describe("installation statistics preferences", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getPreferences.mockResolvedValue({ enabled: true, available: true });
  });

  it("shows the enabled default and allows the user to disable reporting", async () => {
    api.setEnabled.mockResolvedValue({ enabled: false, available: true });
    show();
    const toggle = screen.getByRole("switch", { name: /privacy.statistics/ });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toBeChecked();
    expect(api.setEnabled).not.toHaveBeenCalled();
    await userEvent.click(toggle);
    expect(api.setEnabled).toHaveBeenCalledWith(false, expect.anything());
    await waitFor(() => expect(toggle).not.toBeChecked());
  });

  it("retains the saved disabled choice when settings are reopened", async () => {
    api.getPreferences.mockResolvedValue({ enabled: false, available: true });
    const first = show();
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
    expect(screen.getByRole("switch")).not.toBeChecked();
    first.unmount();
    show();
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(api.setEnabled).not.toHaveBeenCalled();
  });

  it("can enable reporting again after the user has disabled it", async () => {
    api.getPreferences.mockResolvedValue({ enabled: false, available: true });
    api.setEnabled.mockResolvedValue({ enabled: true, available: true });
    show();
    const toggle = screen.getByRole("switch");
    await waitFor(() => expect(toggle).toBeEnabled());
    await userEvent.click(toggle);
    expect(api.setEnabled).toHaveBeenCalledWith(true, expect.anything());
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("allows disabling an enabled preference even without a destination", async () => {
    api.getPreferences.mockResolvedValue({ enabled: true, available: false });
    api.setEnabled.mockResolvedValue({ enabled: false, available: false });
    show();
    const toggle = screen.getByRole("switch");
    await waitFor(() => expect(toggle).toBeEnabled());
    await userEvent.click(toggle);
    expect(api.setEnabled).toHaveBeenCalledWith(false, expect.anything());
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(toggle).toBeDisabled();
  });

  it("cannot opt in when this build has no destination", async () => {
    api.getPreferences.mockResolvedValue({ enabled: false, available: false });
    show();
    await screen.findByText("privacy.unavailable");
    expect(screen.getByRole("switch")).toBeDisabled();
    expect(api.setEnabled).not.toHaveBeenCalled();
  });

  it("retains the saved choice when persistence fails", async () => {
    api.getPreferences.mockResolvedValue({ enabled: true, available: true });
    api.setEnabled.mockRejectedValue(new Error("unavailable"));
    show();
    const toggle = screen.getByRole("switch");
    await waitFor(() => expect(toggle).toBeChecked());
    await userEvent.click(toggle);
    await screen.findByRole("alert");
    expect(toggle).toBeChecked();
  });
});
