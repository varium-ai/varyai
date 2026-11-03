import { render, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import { RoutingStage } from "@/components/routing/RoutingStage";
import type { RoutingTrace } from "@/lib/api/routingTrace";

const now = Date.now();
const row: RoutingTrace = {
  id: "stage-fixture",
  startedAt: now - 1000,
  agent: "codex",
  model: "auto/research",
  groupName: "Research",
  strategy: "pace",
  state: "streaming",
  durationMs: 1000,
  candidates: [
    {
      providerId: "fixture",
      providerName: "Synthetic vendor",
      model: "model-a",
      account: {
        id: "key-a",
        kind: "key",
        providerId: "fixture",
        providerName: "Synthetic vendor",
        model: "model-a",
        restUntil: 0,
        windows: [],
      },
    },
    {
      providerId: "fixture",
      providerName: "Synthetic vendor",
      model: "model-a",
      account: {
        id: "key-b",
        kind: "key",
        providerId: "fixture",
        providerName: "Synthetic vendor",
        model: "model-a",
        restUntil: 0,
        windows: [],
      },
    },
    {
      providerId: "account-vendor",
      providerName: "Synthetic account",
      model: "model-a",
      account: {
        id: "account-a",
        kind: "account",
        providerId: "account-vendor",
        providerName: "Synthetic account",
        model: "model-a",
        restUntil: 0,
        windows: [
          {
            usedPercent: 98,
            windowSeconds: 18000,
            resetsAt: now / 1000 + 3600,
          },
        ],
      },
    },
  ],
  steps: [
    { kind: "attempt", candidate: 1, atMs: 1 },
    { kind: "connected", candidate: 1, atMs: 20, status: 200 },
  ],
};
function show(overrides: Partial<Parameters<typeof RoutingStage>[0]> = {}) {
  return render(
    <RoutingStage
      row={row}
      accountName={(c) => c.providerName}
      animate={false}
      replay={false}
      speed={1}
      now={now}
      address="127.0.0.1:15722"
      requestedModel="auto/research"
      {...overrides}
    />,
  );
}
it("renders individual safe key seats, the requested alias, actual listener and strategy", () => {
  show();
  const src = document.querySelector(".rt-src")!;
  expect(src).toHaveTextContent("apps.codex");
  expect(src).toHaveTextContent("auto/research");
  expect(document.querySelector(".rt-hub")).toHaveTextContent(
    "127.0.0.1:15722",
  );
  expect(document.querySelector(".rt-hub i")).toHaveTextContent(
    "routingPage.strategy.pace",
  );
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats).toHaveLength(3);
  expect(seats[0]).toHaveTextContent("routingPage.keyLabel");
  expect(seats[1]).toHaveClass("on", "now");
  expect(seats[1]).toHaveTextContent("routingPage.seatAnswering");
  expect(seats[0].textContent).not.toContain("key-a");
  expect(seats[0]).not.toHaveAttribute("title", "key-a");
  const quota = within(seats[2] as HTMLElement).getByRole("progressbar");
  expect(quota).toHaveAttribute("aria-valuenow", "2");
  expect(seats[2]).toHaveTextContent("routingPage.seatQuota");
  expect(within(seats[0] as HTMLElement).queryByRole("progressbar")).toBeNull();
});
it("reports early failure rather than pretending to wait for candidates", () => {
  show({
    row: {
      ...row,
      state: "failed",
      status: 409,
      candidates: [],
      steps: [{ kind: "failed", atMs: 2, status: 409 }],
    },
  });
  expect(
    screen.getByText("routingPage.noCandidatesFailed"),
  ).toBeInTheDocument();
  expect(
    screen.queryByText("routingPage.emptyCandidates"),
  ).not.toBeInTheDocument();
  expect(document.querySelector(".rt-accts")).toHaveTextContent("409");
});
it("distinguishes a retry from the final error and does not animate completed seats", () => {
  show({
    row: {
      ...row,
      state: "completed",
      steps: [
        { kind: "attempt", candidate: 0, atMs: 1 },
        { kind: "rejected", candidate: 0, atMs: 10, status: 502 },
        { kind: "attempt", candidate: 1, atMs: 11 },
        { kind: "completed", candidate: 1, atMs: 20, status: 200 },
      ],
    },
  });
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats[0]).toHaveTextContent("routingPage.seatWentOn");
  expect(seats[0]).toHaveTextContent("502");
  expect(seats[1]).toHaveTextContent("routingPage.seatAnswered");
  expect(document.querySelector(".rt-accts .now")).toBeNull();
  expect(document.querySelector(".rt-wires .live")).toBeNull();
});
it("only uses fresh observations for the exact account and model while live", () => {
  const fresh = {
    ...row.candidates[2].account!,
    windows: [
      { usedPercent: 25, windowSeconds: 18000, resetsAt: now / 1000 + 7200 },
    ],
  };
  const { rerender } = show({
    currentAccounts: [fresh],
    liveObservations: true,
  });
  expect(screen.getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "75",
  );
  rerender(
    <RoutingStage
      row={row}
      accountName={(c) => c.providerName}
      animate={false}
      replay={true}
      speed={1}
      now={now}
      currentAccounts={[fresh]}
      liveObservations={false}
    />,
  );
  expect(screen.getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "2",
  );
});
it("does not turn an untried seat busy from an unrelated request", () => {
  const another: RoutingTrace = {
    ...row,
    id: "another",
    steps: [{ kind: "attempt", candidate: 0, atMs: 1 }],
  };
  show({ concurrent: [another], liveObservations: true });
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats[0]).toHaveTextContent("routingPage.seatAnsweringOther");
  expect(seats[0]).not.toHaveClass("on");
  expect(seats[1]).toHaveClass("on");
});
