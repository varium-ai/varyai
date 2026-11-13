import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settingsApi } from "@/lib/api";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";
import type { UpdateInstallProgress, UpdateInstallResult } from "@/lib/updater";

vi.mock("@/lib/api", () => ({
  settingsApi: {
    installUpdateAndRestart: vi.fn(),
    cancelUpdateDownload: vi.fn(),
  },
}));
vi.mock("@/lib/updater", () => ({ checkForUpdate: vi.fn() }));

function Probe() {
  const state = useUpdate();
  return (
    <div>
      <span>{state.hasUpdate ? "available" : "none"}</span>
      <span>{state.isInstalling ? "busy" : "idle"}</span>
      <span>{JSON.stringify(state.installProgress)}</span>
      <span>{state.isDownloadStalled ? "stalled" : "active"}</span>
      <span>{state.error ?? "no error"}</span>
      <button onClick={() => void state.installUpdate().catch(() => {})}>
        install
      </button>
      <button onClick={() => void state.cancelUpdate().catch(() => {})}>
        cancel
      </button>
    </div>
  );
}

let progress: (value: UpdateInstallProgress) => void;
let finish: (value: UpdateInstallResult) => void;
async function mount() {
  render(
    <UpdateProvider>
      <Probe />
    </UpdateProvider>,
  );
  await act(() => vi.advanceTimersByTimeAsync(1000));
  await act(async () => {
    fireEvent.click(screen.getByText("install"));
  });
}

describe("UpdateProvider", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    vi.mocked(checkForUpdate).mockResolvedValue({
      status: "available",
      info: { currentVersion: "0.2.1", availableVersion: "0.2.2" },
    });
    vi.mocked(settingsApi.installUpdateAndRestart).mockImplementation(
      (_id, onProgress) => {
        progress = onProgress;
        return new Promise((resolve) => {
          finish = resolve;
        });
      },
    );
    vi.mocked(settingsApi.cancelUpdateDownload).mockResolvedValue(true);
  });
  afterEach(() => {
    vi.useRealTimers();
    localStorage.clear();
  });

  it("tracks checking, byte progress and installation without allowing duplicate installs", async () => {
    await mount();
    expect(screen.getByText("busy")).toBeVisible();
    expect(screen.getByText(/\"phase\":\"checking\"/)).toBeVisible();
    act(() => progress({ phase: "downloading", downloaded: 512, total: 1024 }));
    expect(screen.getByText(/\"downloaded\":512/)).toBeVisible();
    fireEvent.click(screen.getByText("install"));
    expect(settingsApi.installUpdateAndRestart).toHaveBeenCalledTimes(1);
    act(() => progress({ phase: "installing", downloaded: 1024, total: 1024 }));
    fireEvent.click(screen.getByText("cancel"));
    expect(settingsApi.cancelUpdateDownload).not.toHaveBeenCalled();
    await act(async () => finish("installed"));
    expect(screen.getByText("idle")).toBeVisible();
    expect(screen.getByText("none")).toBeVisible();
  });

  it("shows a stalled download hint until more data arrives, including unknown lengths", async () => {
    await mount();
    act(() => progress({ phase: "downloading", downloaded: 12, total: null }));
    await act(() => vi.advanceTimersByTimeAsync(15000));
    expect(screen.getByText("stalled")).toBeVisible();
    act(() => progress({ phase: "downloading", downloaded: 24, total: null }));
    expect(screen.getByText("active")).toBeVisible();
    act(() => progress({ phase: "installing", downloaded: 24, total: null }));
    await act(() => vi.advanceTimersByTimeAsync(15000));
    expect(screen.getByText("active")).toBeVisible();
    await act(async () => finish("installed"));
  });

  it("cancels the matching attempt, retains the update for retry and ignores late callbacks", async () => {
    await mount();
    act(() => progress({ phase: "downloading", downloaded: 12, total: null }));
    await act(async () => fireEvent.click(screen.getByText("cancel")));
    expect(settingsApi.cancelUpdateDownload).toHaveBeenCalledWith(
      vi.mocked(settingsApi.installUpdateAndRestart).mock.calls[0][0],
    );
    expect(screen.getByText("busy")).toBeVisible();
    const oldProgress = progress;
    await act(async () => finish("cancelled"));
    expect(screen.getByText("idle")).toBeVisible();
    expect(screen.getByText("available")).toBeVisible();
    expect(screen.getByText("no error")).toBeVisible();
    await act(async () => fireEvent.click(screen.getByText("install")));
    act(() =>
      oldProgress({ phase: "installing", downloaded: 999, total: 999 }),
    );
    expect(screen.getByText(/\"phase\":\"checking\"/)).toBeVisible();
    await act(async () => finish("installed"));
  });

  it("retains an available update after a failed download and permits retry", async () => {
    vi.mocked(settingsApi.installUpdateAndRestart).mockRejectedValueOnce(
      new Error("synthetic failure"),
    );
    await mount();
    expect(screen.getByText("idle")).toBeVisible();
    expect(screen.getByText("available")).toBeVisible();
    await act(async () => fireEvent.click(screen.getByText("install")));
    expect(screen.getByText("busy")).toBeVisible();
    await act(async () => finish("up-to-date"));
    expect(screen.getByText("none")).toBeVisible();
  });
});
