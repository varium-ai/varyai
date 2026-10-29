import type { ReactNode } from "react";
import {
  act,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { toast } from "sonner";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
import { Toaster } from "@/components/ui/sonner";

const api = vi.hoisted(() => ({ switch: vi.fn() }));
vi.mock("@/lib/api", () => ({
  providersApi: api,
  sessionsApi: {},
  settingsApi: {},
}));
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "light" }),
}));

it("shows a standard dismissible switch failure without a copy action", async () => {
  const user = userEvent.setup();
  api.switch.mockRejectedValue(new Error("Synthetic native bridge failure"));
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  render(<Toaster />);
  const { result } = renderHook(() => useSwitchProviderMutation("trae"), {
    wrapper,
  });
  await act(async () => {
    await result.current
      .mutateAsync("synthetic-provider")
      .catch(() => undefined);
  });
  const closeButton = await screen.findByRole("button", {
    name: /关闭|close/i,
  });
  expect(
    screen.queryByRole("button", { name: /复制|copy/i }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByText(/Synthetic native bridge failure/),
  ).toBeInTheDocument();
  expect(closeButton.closest("[data-sonner-toast]")).toHaveAttribute(
    "data-removed",
    "false",
  );
  await user.click(closeButton);
  await waitFor(() =>
    expect(
      screen.queryByText(/Synthetic native bridge failure/),
    ).not.toBeInTheDocument(),
  );
  toast.dismiss();
});
