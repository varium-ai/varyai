import { render, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import zh from "@/i18n/locales/zh.json";
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
it.each([
  ["routing", true, false, true],
  ["streaming", true, false, true],
  ["completed", true, false, false],
  ["cancelled", true, false, false],
  ["streaming", false, false, false],
  ["streaming", true, true, false],
] as const)(
  "animates the gateway only for live activity: %s, %s, %s",
  (state, animate, replay, active) => {
    show({ row: { ...row, state }, animate, replay });
    expect(
      document.querySelector(".rt-hub")?.classList.contains("working"),
    ).toBe(active);
  },
);

it("keeps the gateway active while an older concurrent request is streaming", () => {
  show({
    row: { ...row, id: "finished", state: "completed" },
    concurrent: [row],
    animate: true,
  });
  expect(document.querySelector(".rt-hub")).toHaveClass("working");
});

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
  expect(src.querySelector("small")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub b")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub small")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub i")).not.toHaveClass("sr-only");
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats).toHaveLength(3);
  expect(seats[0]).toHaveTextContent("routingPage.apiKey");
  expect(seats[1]).toHaveClass("on", "now");
  expect(seats[1]).toHaveTextContent("routingPage.seatAnswering");
  expect(seats[0].textContent).not.toContain("key-a");
  expect(seats[0]).not.toHaveAttribute("title", "key-a");
  const quota = within(seats[2] as HTMLElement).getByRole("progressbar");
  expect(quota).toHaveAttribute("aria-valuenow", "2");
  expect(seats[2]).toHaveTextContent("routingPage.seatQuota");
  expect(within(seats[0] as HTMLElement).queryByRole("progressbar")).toBeNull();
});
it("names provider seats by their supplier even when legacy labels remain", () => {
  show({
    row: {
      ...row,
      candidates: [
        {
          ...row.candidates[0],
          account: {
            ...row.candidates[0].account!,
            kind: "provider",
            label: "sk-4…766b",
          },
        },
      ],
    },
  });
  expect(document.querySelector(".rt-accts .who")).toHaveTextContent(
    "Synthetic vendor",
  );
  expect(document.querySelector(".rt-accts .who")).not.toHaveTextContent(
    "sk-4…766b",
  );
});

it("shows the supplier for a legacy key label when only one configured key is enabled", () => {
  show({
    row: {
      ...row,
      candidates: [
        {
          ...row.candidates[0],
          account: { ...row.candidates[0].account!, label: "sk-4…766b" },
        },
      ],
    },
    upstreams: [
      {
        schemaVersion: 1,
        id: "fixture",
        name: "Synthetic vendor",
        revision: 1,
        hasApiKey: true,
        source: {
          kind: "api",
          baseUrl: "https://example.invalid",
          apiFormat: "openai_chat",
          models: [],
          catalog: [],
          mode: "vendor",
          groupsOnly: false,
        },
      },
    ],
  });
  expect(document.querySelector(".rt-accts .who")).toHaveTextContent(
    "Synthetic vendor",
  );
  expect(document.querySelector(".rt-accts .who")).not.toHaveTextContent(
    "sk-4…766b",
  );
});

it("uses each candidate's configured supplier icon instead of credential-type icons", () => {
  show({
    upstreams: [
      {
        schemaVersion: 1,
        id: "fixture",
        name: "Synthetic vendor",
        revision: 1,
        hasApiKey: true,
        icon: "deepseek",
        source: { kind: "codex" },
      },
    ],
  });
  const icons = document.querySelectorAll(".rt-seat-icon");
  expect(icons).toHaveLength(3);
  expect(icons[0].querySelector(".lucide-key-round")).toBeNull();
  expect(
    icons[0].querySelector("[title='Synthetic vendor'] svg"),
  ).not.toBeNull();
  expect(icons[0].innerHTML).toBe(icons[1].innerHTML);
  expect(icons[2].querySelector(".lucide-box")).not.toBeNull();
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
  expect(document.querySelectorAll(".rt-wires path")).toHaveLength(1);
  expect(document.querySelectorAll(".rt-packet-path")).toHaveLength(1);
});
it("shows recorded masked key labels instead of provider names and key ordinals", () => {
  show({
    row: {
      ...row,
      candidates: row.candidates.slice(0, 2).map((candidate, index) => ({
        ...candidate,
        account: {
          ...candidate.account!,
          label: index ? "sk-8…cd3e" : "sk-4…766b",
        },
      })),
    },
  });
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats[0].querySelector(".who")).toHaveTextContent("sk-4…766b");
  expect(seats[1].querySelector(".who")).toHaveTextContent("sk-8…cd3e");
  expect(seats[0].querySelector(".who")).not.toHaveTextContent(
    "Synthetic vendor",
  );
  expect(seats[0].querySelector(".who")).not.toHaveTextContent(
    "routingPage.keyLabel",
  );
  expect(seats[0]).toHaveAttribute(
    "title",
    expect.stringContaining("sk-4…766b"),
  );
});
it.each([true, false])(
  "hides HTTP 499 for cancellation with candidates: %s",
  (hasCandidates) => {
    show({
      row: {
        ...row,
        state: "cancelled",
        status: 499,
        candidates: hasCandidates ? row.candidates : [],
        steps: [
          {
            kind: "cancelled",
            candidate: hasCandidates ? 1 : undefined,
            atMs: 1000,
            status: 499,
          },
        ],
      },
    });
    expect(document.querySelector(".rt-accts")).not.toHaveTextContent("499");
    expect(document.querySelector(".rt-accts .rest")).toBeNull();
    if (!hasCandidates)
      expect(
        screen.getByText("routingPage.noCandidatesCancelled"),
      ).toBeInTheDocument();
  },
);
it("keeps the tray stage horizontal while the workspace stage folds at narrow widths", () => {
  const view = show({ layout: "compact" });
  expect(document.querySelector(".rt-stage")).not.toHaveClass("narrow");
  expect(document.querySelector(".rt-stage")).toHaveClass("compact");
  expect(document.querySelector(".rt-hub b")).toHaveClass("sr-only");
  expect(document.querySelector(".rt-src small")).toHaveClass("sr-only");
  view.unmount();
  show();
  expect(document.querySelector(".rt-stage")).toHaveClass("narrow");
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
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "2");
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

it("shows an idle placeholder before any request has been recorded", () => {
  show({ row: undefined });
  expect(document.querySelectorAll(".rt-src")).toHaveLength(1);
  expect(document.querySelector(".rt-src")).toHaveTextContent(
    "routingPage.agent",
  );
  expect(document.querySelectorAll(".rt-accts li:not(.idle)")).toHaveLength(0);
  expect(screen.getByText("routingPage.emptyCandidates")).toBeInTheDocument();
  expect(document.querySelector(".rt-agent-icon svg")).toBeInTheDocument();
  expect(document.querySelector(".rt-src small")).not.toBeInTheDocument();
  expect(document.querySelector(".rt-hub b")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub small")).toHaveTextContent(
    "127.0.0.1:15722",
  );
});

it("shows one branch for a direct request and preserves every recorded fallback for a group", () => {
  const view = show({
    row: { ...row, candidates: [row.candidates[0]], steps: [] },
  });
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(1);
  expect(document.querySelector(".rt-accts")).toHaveTextContent("model-a");
  view.unmount();
  show();
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(3);
});

it("keeps simultaneous request paths on stage when the latest request changes", () => {
  const older: RoutingTrace = {
    ...row,
    candidates: [row.candidates[0]],
    steps: [{ kind: "attempt", candidate: 0, atMs: 1 }],
  };
  const view = show({ row: older, animate: true, concurrent: [older] });
  const packet = document.querySelector(".rt-packet");
  const next: RoutingTrace = {
    ...older,
    id: "newer-stage-request",
    agent: "gemini",
    candidates: [row.candidates[2]],
  };
  view.rerender(
    <RoutingStage
      row={next}
      accountName={(candidate) => candidate.providerName}
      concurrent={[next, older]}
      animate
      replay={false}
      speed={1}
    />,
  );
  expect(document.querySelectorAll(".rt-src")).toHaveLength(2);
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
  expect(document.querySelectorAll(".rt-packet")).toHaveLength(2);
  expect([...document.querySelectorAll(".rt-packet")].includes(packet!)).toBe(
    true,
  );
  view.rerender(
    <RoutingStage
      row={next}
      accountName={(candidate) => candidate.providerName}
      concurrent={[next, older]}
      animate={false}
      replay={false}
      speed={1}
    />,
  );
  expect(document.querySelectorAll(".rt-src")).toHaveLength(1);
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(1);
});

it("shows one node per shared candidate without duplicating a concurrent branch", () => {
  const view = show({ row, animate: true, concurrent: [row] });
  const next = { ...row, id: "same-route-next" };
  view.rerender(
    <RoutingStage
      row={next}
      accountName={(candidate) => candidate.providerName}
      concurrent={[next, row]}
      animate
      replay={false}
      speed={1}
    />,
  );
  expect(document.querySelectorAll(".rt-src")).toHaveLength(1);
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(3);
  expect(document.querySelectorAll(".rt-packet")).toHaveLength(2);
});

it("uses unnumbered key status without repeating credential ordinals", () => {
  show({
    row: {
      ...row,
      state: "cancelled",
      candidates: row.candidates.slice(0, 2).map((candidate, index) => ({
        ...candidate,
        account: { ...candidate.account!, label: `Masked key ${index + 1}` },
      })),
      steps: [],
    },
  });
  for (const status of document.querySelectorAll(".rt-seat-status")) {
    expect(status).toHaveTextContent("routingPage.apiKey");
    expect(status).not.toHaveTextContent("routingPage.keyLabel");
  }
});

it("shows recorded subscription plans beside names without guessing a plan for keys", () => {
  show({
    row: {
      ...row,
      candidates: row.candidates.map((candidate) => ({
        ...candidate,
        account: { ...candidate.account!, plan: "ChatGPT Plus" },
      })),
    },
  });
  const seats = document.querySelectorAll(".rt-accts li");
  expect(seats[2].querySelector("b")).toHaveTextContent("ChatGPT Plus");
  expect(seats[0].querySelector("b")).not.toHaveTextContent("ChatGPT Plus");
});

it("keeps renewal information alongside an answering account's status", () => {
  show({
    row: { ...row, steps: [{ kind: "attempt", candidate: 2, atMs: 1 }] },
  });
  const status = document.querySelectorAll(".rt-seat-status")[2];
  expect(status).toHaveTextContent("routingPage.seatAnswering");
  expect(status).toHaveTextContent("routingPage.resetIn");
});

it("prioritizes passed-over context before active and quota states", () => {
  show({
    row: {
      ...row,
      steps: [{ kind: "context", candidate: 2, atMs: 1 }],
    },
  });
  const status = document.querySelectorAll(".rt-seat-status")[2];
  expect(status).toHaveTextContent("routingPage.step.context");
  expect(status).not.toHaveTextContent("routingPage.seatQuota");
});

it.each(["responsive", "compact"] as const)(
  "shows a rounded multi-day reset in the %s stage and tooltip",
  async (layout) => {
    const locale = createInstance();
    await locale.init({ lng: "zh", resources: { zh: { translation: zh } } });
    const candidate = row.candidates[2];
    const trace: RoutingTrace = {
      ...row,
      startedAt: now,
      durationMs: 0,
      candidates: [
        {
          ...candidate,
          account: {
            ...candidate.account!,
            windows: [
              {
                usedPercent: 25,
                windowSeconds: 604800,
                resetsAt: now / 1000 + (88 * 60 + 46) * 60,
              },
            ],
          },
        },
      ],
      steps: [{ kind: "attempt", candidate: 0, atMs: 1 }],
    };
    render(
      <I18nextProvider i18n={locale}>
        <RoutingStage
          row={trace}
          layout={layout}
          now={now}
          liveObservations
          accountName={(c) => c.providerName}
          animate={false}
          replay={false}
          speed={1}
        />
      </I18nextProvider>,
    );
    const seat = document.querySelector(".rt-accts li")!;
    expect(seat.querySelector(".rt-seat-status")).toHaveTextContent(
      "4 天后重置",
    );
    expect(seat.getAttribute("title")).toContain("4 天后重置");
    expect(seat).not.toHaveTextContent("88 小时");
  },
);

it("resolves a renamed supplier's icon by its public identity while retaining execution identity", () => {
  show({
    row: {
      ...row,
      candidates: [
        {
          ...row.candidates[0],
          providerId: "immutable-owner",
          sourceProviderId: "renamed-supplier",
        },
      ],
    },
    upstreams: [
      {
        schemaVersion: 1,
        id: "renamed-supplier",
        name: "Configured supplier",
        revision: 2,
        hasApiKey: true,
        icon: "deepseek",
        source: { kind: "codex" },
      },
    ],
  });
  expect(document.querySelector(".rt-seat-icon .lucide-box")).toBeNull();
  expect(
    document.querySelector('.rt-seat-icon [title="Configured supplier"] svg'),
  ).not.toBeNull();
});
