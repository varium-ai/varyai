import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkBuddyCheckinPanel } from "@/components/WorkBuddyCheckinPanel";
import {
  workbuddyCheckinApi,
  type WorkBuddyCheckinStatus,
} from "@/lib/api/workbuddy-checkin";

vi.mock("@/lib/api/workbuddy-checkin", () => ({
  workbuddyCheckinApi: {
    status: vi.fn(),
    checkIn: vi.fn(),
    setAutoEnabled: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const initial: WorkBuddyCheckinStatus = {
  autoEnabled: false,
  day: "2026-11-02",
  outcome: "not_checked",
  checkedAt: null,
  credit: null,
  streak: null,
  creditDelayed: false,
};
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <WorkBuddyCheckinPanel />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.mocked(workbuddyCheckinApi.status).mockResolvedValue(initial);
  vi.mocked(workbuddyCheckinApi.checkIn).mockResolvedValue({
    ...initial,
    outcome: "claimed",
    credit: 30,
    streak: 3,
    creditDelayed: true,
  });
  vi.mocked(workbuddyCheckinApi.setAutoEnabled).mockImplementation(
    async (enabled) => ({ ...initial, autoEnabled: enabled }),
  );
});

describe("WorkBuddy daily check-in", () => {
  it("loads local status without claiming, and claims only on a press", async () => {
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    const button = await screen.findByRole("button", {
      name: "workbuddy.checkin.checkNow",
    });
    expect(button).toBeEnabled();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    expect(
      screen.getByRole("switch", { name: "workbuddy.checkin.autoLabel" }),
    ).not.toBeChecked();
    fireEvent.click(button);
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.claimed"),
    ).toBeInTheDocument();
    expect(workbuddyCheckinApi.checkIn).toHaveBeenCalledOnce();
    expect(screen.getByText("workbuddy.checkin.reward")).toBeInTheDocument();
    expect(screen.getByText("workbuddy.checkin.delayed")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeDisabled();
  });

  it("saves automatic check-in independently from the manual action", async () => {
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    const toggle = await screen.findByRole("switch", {
      name: "workbuddy.checkin.autoLabel",
    });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(workbuddyCheckinApi.setAutoEnabled).toHaveBeenCalledWith(true);
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  });

  it("disables both actions during a claim and does not echo raw errors", async () => {
    let reject!: (reason: Error) => void;
    vi.mocked(workbuddyCheckinApi.checkIn).mockReturnValue(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    fireEvent.click(
      await screen.findByRole("button", { name: "workbuddy.checkin.checkNow" }),
    );
    await waitFor(() => expect(screen.getByRole("switch")).toBeDisabled());
    reject(new Error("private-token-from-upstream"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "workbuddy.checkin.operationFailed",
    );
    expect(screen.queryByText(/private-token/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeEnabled();
  });

  it("guides an unreadable login back to the official client", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "credentials_unavailable",
    });
    mount();
    expect(
      await screen.findByText(
        "workbuddy.checkin.outcomes.credentials_unavailable",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeDisabled();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  });

  it("allows a retry after the user renews a rejected login in WorkBuddy", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "sign_in_required",
    });
    mount();
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.sign_in_required"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    );
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.claimed"),
    ).toBeInTheDocument();
  });

  it("shows the own-app requirement without claiming a reward", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "own_app_required",
    });
    mount();
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.own_app_required"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("workbuddy.checkin.reward"),
    ).not.toBeInTheDocument();
  });

  it("offers reload on a local status error without enabling actions", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockRejectedValueOnce(
      new Error("private-state"),
    );
    mount();
    const reload = await screen.findByRole("button", {
      name: "workbuddy.checkin.reload",
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "workbuddy.checkin.loadFailed",
    );
    expect(screen.queryByText(/private-state/)).not.toBeInTheDocument();
    fireEvent.click(reload);
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    expect(
      await screen.findByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeEnabled();
  });
});
