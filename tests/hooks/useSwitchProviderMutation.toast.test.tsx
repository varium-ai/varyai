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

it("keeps a switch failure open after copying and provides a separate close button", async () => {
  const user = userEvent.setup();
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
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
  const copyButton = await screen.findByRole("button", { name: /复制|copy/i });
  await user.click(copyButton);
  expect(copy).toHaveBeenCalledWith("Synthetic native bridge failure");
  expect(copyButton.closest("[data-sonner-toast]")).toHaveAttribute(
    "data-removed",
    "false",
  );
  await user.click(screen.getByRole("button", { name: /关闭|close/i }));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: /复制|copy/i }),
    ).not.toBeInTheDocument(),
  );
  toast.dismiss();
});
