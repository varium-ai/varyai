import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { settingsApi } from "@/lib/api";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@/lib/api", () => ({
  settingsApi: {
    installUpdateAndRestart: vi.fn(),
  },
}));

vi.mock("@/lib/updater", () => ({
  checkForUpdate: vi.fn(),
}));

function UpdateContextProbe() {
  const { hasUpdate, isInstalling, installUpdate } = useUpdate();

  return (
    <div>
      <span>{hasUpdate ? "available" : "none"}</span>
      <span>{isInstalling ? "installing" : "idle"}</span>
      <button onClick={() => void installUpdate()}>install</button>
    </div>
  );
}

describe("UpdateProvider", () => {
  afterEach(() => {
    vi.useRealTimers();
    localStorage.clear();
  });

  it("installs the detected update and reports its in-progress state", async () => {
    vi.useFakeTimers();
    vi.mocked(checkForUpdate).mockResolvedValue({
      status: "available",
      info: {
        currentVersion: "0.2.1",
        availableVersion: "0.2.2",
        notes: "Fixes and improvements",
      },
    });

    let finishInstall!: (result: boolean) => void;
    vi.mocked(settingsApi.installUpdateAndRestart).mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          finishInstall = resolve;
        }),
    );

    render(
      <UpdateProvider>
        <UpdateContextProbe />
      </UpdateProvider>,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByText("available")).toBeInTheDocument();

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "install" }));
    });
    expect(settingsApi.installUpdateAndRestart).toHaveBeenCalledTimes(1);
    expect(screen.getByText("installing")).toBeInTheDocument();

    await act(async () => {
      finishInstall(true);
    });
    expect(screen.getByText("idle")).toBeInTheDocument();
  });
});
