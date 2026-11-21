import { readFileSync } from "node:fs";
import { render, screen, within } from "@testing-library/react";
import postcss from "postcss";
import { expect, it, vi } from "vitest";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import zh from "@/i18n/locales/zh.json";
import { APP_IDS } from "@/config/appConfig";
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
      model: "unused-model",
      account: {
        id: "account-a",
        kind: "account",
        providerId: "account-vendor",
        providerName: "Synthetic account",
        model: "unused-model",
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
const attempt = (candidate: number) => ({
  kind: "attempt" as const,
  candidate,
  atMs: 0,
});
const allSeatsRow = {
  ...row,
  steps: [attempt(0), attempt(2), ...row.steps],
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

it("pulses between two border colors on the fixed card outline only when motion is enabled", () => {
  const stylesheet = postcss.parse(
    readFileSync("src/components/routing/routing.css", "utf8"),
  );
  const outline = ".vary-routing .rt-stage .rt-accts li.on.replying::after";
  let motionRule = false;
  let inset: string | undefined;
  let border: string | undefined;
  stylesheet.walkAtRules("media", (media) => {
    if (media.params !== "(prefers-reduced-motion: no-preference)") return;
    media.walkRules(outline, (rule) => {
      rule.walkDecls("animation", ({ value }) => {
        motionRule = value.startsWith("vary-rt-seat-pulse ");
      });
      rule.walkDecls("inset", ({ value }) => {
        inset = value;
      });
      rule.walkDecls("border", ({ value }) => {
        border = value;
      });
    });
  });
  const animatedProperties = new Set<string>();
  const colors = new Set<string>();
  stylesheet.walkAtRules("keyframes", (animation) => {
    if (animation.params !== "vary-rt-seat-pulse") return;
    animation.walkDecls(({ prop, value }) => {
      animatedProperties.add(prop);
      if (prop === "border-color") colors.add(value);
    });
  });
  expect(motionRule).toBe(true);
  expect(inset).toBe("-1px");
  expect(border).toMatch(/^2px solid\b/);
  expect([...animatedProperties]).toEqual(["border-color"]);
  expect(colors.size).toBe(2);
});

it.each(
  APP_IDS.flatMap((agent) =>
    (["responsive", "compact"] as const).map((layout) => ({ agent, layout })),
  ),
)(
  "only shows the attempted destination for $agent in $layout",
  ({ agent, layout }) => {
    const request = { ...row, agent, sessionTitle: "Synthetic conversation" };
    const view = show({
      row: { ...request, steps: [] },
      layout,
      animate: true,
    });
    const update = (changes: Partial<RoutingTrace>, replay = false) =>
      view.rerender(
        <RoutingStage
          row={{ ...request, ...changes }}
          layout={layout}
          accountName={(candidate) => candidate.providerName}
          animate
          speed={1}
          replay={replay}
        />,
      );
    const seats = () =>
      view.container.querySelectorAll(".rt-accts li:not(.idle)");
    const highlighted = () =>
      view.container.querySelectorAll(".rt-accts .replying");
    expect(highlighted()).toHaveLength(0);
    expect(seats()).toHaveLength(0);
    update({ firstTokenMs: null });
    expect(highlighted()).toHaveLength(1);
    expect(seats()).toHaveLength(1);
    expect(seats()[0]).toHaveClass("replying");
    expect(seats()[0]).toHaveTextContent("model-a");
    expect(seats()[0]).toHaveTextContent("Synthetic conversation");
    expect(seats()[0].querySelector(".rt-seat-status")).toHaveClass(
      "has-response",
    );
    expect(seats()[0]).not.toHaveTextContent("routingPage.outputRate");
    update({ outputTokensPerSecond: 42.5 });
    expect(seats()[0]).toHaveTextContent("routingPage.outputRate");
    expect(view.container.querySelector(".rt-accts .dot")).toBeNull();
    update({}, true);
    expect(highlighted()).toHaveLength(0);
    update({
      firstTokenMs: 0,
      state: "completed",
      outputTokensPerSecond: 42.5,
    });
    expect(highlighted()).toHaveLength(0);
    expect(seats()[0]).toHaveTextContent("routingPage.seatAnswered");
    expect(seats()[0]).toHaveTextContent("Synthetic conversation");
    expect(seats()[0]).toHaveTextContent("routingPage.outputRate");
  },
);

it("shows only the winning request's title and output rate, never fallback or account metrics", () => {
  const observed = {
    ...row,
    sessionTitle: "Synthetic selected conversation",
    outputTokensPerSecond: 48,
    steps: [
      attempt(0),
      { kind: "rejected" as const, candidate: 0, atMs: 1 },
      ...row.steps,
    ],
  };
  show({
    row: observed,
    sources: [
      observed,
      {
        ...observed,
        id: "completed-peer",
        sessionTitle: "Unrelated conversation",
        state: "completed",
        outputTokensPerSecond: 15,
      },
    ],
    currentAccounts: [
      { ...row.candidates[0].account!, outputTokensPerSecond: 100 },
    ],
    liveObservations: true,
    animate: true,
  });
  const [fallback, winner] = document.querySelectorAll(".rt-seat-status");
  expect(fallback).not.toHaveClass("has-response");
  expect(fallback).not.toHaveTextContent("Synthetic selected conversation");
  expect(fallback).not.toHaveTextContent("routingPage.outputRate");
  expect(winner).toHaveTextContent("Synthetic selected conversation");
  expect(winner).toHaveTextContent("routingPage.outputRate");
  expect(winner).not.toHaveTextContent("Unrelated conversation");
});

it("renders each concurrent response's localized title and measured rate on its card", async () => {
  const locale = createInstance();
  await locale.init({ lng: "zh", resources: { zh: { translation: zh } } });
  const request = {
    ...row,
    sessionTitle: "合成测试会话",
    outputTokensPerSecond: 42.49,
    steps: [attempt(2)],
  };
  const otherRequest = {
    ...row,
    id: "stage-peer",
    agent: "claude",
    sessionTitle: "另一条测试会话",
    outputTokensPerSecond: 17.1,
  };
  const view = render(
    <I18nextProvider i18n={locale}>
      <RoutingStage
        row={request}
        sources={[request, otherRequest]}
        accountName={(candidate) => candidate.providerName}
        layout="compact"
        now={now}
        animate={false}
        replay={false}
        speed={1}
      />
    </I18nextProvider>,
  );
  const [first, second] = view.container.querySelectorAll(
    ".rt-seat-status.has-response",
  );
  expect(first).toHaveTextContent(
    /正在回答.*会话名称：合成测试会话 · 42\.5 tok\/s/,
  );
  expect(first?.closest("li")).toHaveAttribute(
    "title",
    expect.stringContaining("合成测试会话 · 42.5 tok/s"),
  );
  expect(second).toHaveTextContent(
    /正在回答.*会话名称：另一条测试会话 · 17\.1 tok\/s/,
  );
});

it.each([0, -3, Infinity, NaN, undefined])(
  "omits unavailable output rate %s and uses an opaque session reference",
  (rate) => {
    const digest = "ab123456".repeat(8);
    show({ row: { ...row, session: digest, outputTokensPerSecond: rate } });
    expect(document.querySelector(".rt-seat-status")).toHaveTextContent(
      "routingPage.sessionReference",
    );
    expect(document.querySelector(".rt-seat-status")).not.toHaveTextContent(
      digest,
    );
    expect(document.querySelector(".rt-seat-status")).not.toHaveTextContent(
      "routingPage.outputRate",
    );
  },
);

it("keeps a shared seat highlighted when its other request is complete", () => {
  show({
    row,
    sources: [row, { ...row, id: "completed-peer", state: "completed" }],
    animate: true,
  });
  expect(document.querySelectorAll(".rt-accts .replying")).toHaveLength(1);
});

it("does not highlight an abandoned target after it recorded content", () => {
  show({
    row: {
      ...row,
      firstTokenMs: 24,
      steps: [...row.steps, { kind: "failed", candidate: 1, atMs: 30 }],
    },
    animate: true,
  });
  expect(document.querySelector(".rt-accts .replying")).toBeNull();
});
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

it("renders individual safe key seats, the requested alias and the listener without a strategy label", () => {
  show({ row: allSeatsRow });
  const src = document.querySelector(".rt-src")!;
  expect(src).toHaveTextContent("apps.codex");
  expect(src).toHaveTextContent("auto/research");
  expect(document.querySelector(".rt-hub")).toHaveTextContent(
    "127.0.0.1:15722",
  );
  expect(document.querySelector(".rt-hub i")).toBeNull();
  expect(src.querySelector("small")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub b")).not.toHaveClass("sr-only");
  expect(document.querySelector(".rt-hub small")).not.toHaveClass("sr-only");
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
      steps: [attempt(0)],
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
      steps: [attempt(0)],
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
    row: allSeatsRow,
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
      steps: [attempt(0), ...row.steps],
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
          ...(hasCandidates ? [attempt(1)] : []),
          {
            kind: "cancelled",
            candidate: hasCandidates ? 1 : undefined,
            atMs: 1000,
            status: 499,
          },
        ],
      },
    });
    expect(document.querySelectorAll(".rt-accts li:not(.idle)")).toHaveLength(
      hasCandidates ? 1 : 0,
    );
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
  expect(seats).toHaveLength(2);
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
    row: allSeatsRow,
    currentAccounts: [fresh],
    liveObservations: true,
  });
  expect(screen.getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "75",
  );
  rerender(
    <RoutingStage
      row={allSeatsRow}
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
it("shows other activity on an attempted seat without highlighting it as selected", () => {
  const another: RoutingTrace = {
    ...row,
    id: "another",
    steps: [{ kind: "attempt", candidate: 0, atMs: 1 }],
  };
  show({
    row: { ...row, steps: [attempt(0), ...row.steps] },
    concurrent: [another],
    liveObservations: true,
  });
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

it("does not draw destinations skipped before dispatch or referenced by invalid attempts", () => {
  show({
    row: {
      ...row,
      state: "failed",
      status: 400,
      steps: [
        { kind: "context", candidate: 0, atMs: 1 },
        { kind: "images", candidate: 1, atMs: 2 },
        { kind: "attempt", candidate: null, atMs: 3 },
        { kind: "attempt", candidate: -1, atMs: 4 },
        { kind: "attempt", candidate: 0.5, atMs: 5 },
        { kind: "attempt", candidate: 99, atMs: 6 },
        { kind: "failed", atMs: 7, status: 400 },
      ],
    },
  });
  expect(document.querySelectorAll(".rt-accts li:not(.idle)")).toHaveLength(0);
  expect(document.querySelectorAll(".rt-wires path")).toHaveLength(1);
  expect(
    screen.getByText("routingPage.noCandidatesFailed"),
  ).toBeInTheDocument();
  expect(document.querySelector(".rt-accts")).toHaveTextContent("400");
});

it.each(["responsive", "compact"] as const)(
  "keeps the packet wire at the original candidate index in the %s stage",
  (layout) => {
    const bounds = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        if (this.matches(".rt-src")) return new DOMRect(0, 10, 120, 40);
        if (this.matches(".rt-hub")) return new DOMRect(180, 10, 60, 40);
        if (this.matches(".rt-accts li")) return new DOMRect(320, 10, 140, 40);
        return new DOMRect(0, 0, 700, 120);
      });
    try {
      const view = show({ layout });
      const wires = view.container.querySelectorAll(".rt-wires path");
      const packetWires = view.container.querySelectorAll(".rt-packet-path");
      expect(wires).toHaveLength(2);
      expect(packetWires).toHaveLength(4);
      expect(wires[1].getAttribute("d")).toMatch(/320,30$/);
      expect(packetWires[1]).toHaveAttribute("d", "");
      expect(packetWires[2]).toHaveAttribute("d", wires[1].getAttribute("d"));
      expect(packetWires[3]).toHaveAttribute("d", "");
    } finally {
      bounds.mockRestore();
    }
  },
);

it("keeps simultaneous request paths on stage when the latest request changes", () => {
  const older: RoutingTrace = {
    ...row,
    candidates: [row.candidates[0], row.candidates[2]],
    steps: [{ kind: "attempt", candidate: 0, atMs: 1 }],
  };
  const view = show({ row: older, animate: true, concurrent: [older] });
  const packet = document.querySelector(".rt-packet");
  const next: RoutingTrace = {
    ...older,
    id: "newer-stage-request",
    agent: "gemini",
    candidates: [
      { ...row.candidates[2], model: "other-model" },
      { ...row.candidates[0], model: "another-unused-model" },
    ],
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
  expect(
    [...document.querySelectorAll(".rt-accts .mdl")]
      .map((model) => model.textContent)
      .sort(),
  ).toEqual(["model-a", "other-model"]);
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

it("clears a completed model from another group while the next request is being planned", () => {
  const previous = { ...row, state: "completed" as const };
  const next: RoutingTrace = {
    ...row,
    id: "next-group-request",
    model: "vary-auto/next-group",
    state: "routing",
    candidates: [],
    steps: [],
  };
  const view = show({
    row: previous,
    sources: [previous],
    concurrent: [previous],
    animate: true,
  });
  expect(view.container.querySelector(".rt-accts .mdl")).toBeInTheDocument();
  view.rerender(
    <RoutingStage
      row={next}
      accountName={(candidate) => candidate.providerName}
      sources={[next]}
      concurrent={[next, previous]}
      animate
      replay={false}
      speed={1}
    />,
  );
  expect(view.container.querySelector(".rt-accts .mdl")).toBeNull();
});

it("shows one node per shared destination without duplicating a concurrent branch", () => {
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
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(1);
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
      steps: [
        attempt(0),
        attempt(1),
        { kind: "cancelled", candidate: 1, atMs: 1 },
      ],
    },
  });
  expect(document.querySelectorAll(".rt-seat-status")).toHaveLength(2);
  for (const status of document.querySelectorAll(".rt-seat-status")) {
    expect(status).toHaveTextContent("routingPage.apiKey");
    expect(status).not.toHaveTextContent("routingPage.keyLabel");
  }
});

it("shows recorded subscription plans beside names without guessing a plan for keys", () => {
  show({
    row: {
      ...allSeatsRow,
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
  const status = document.querySelector(".rt-seat-status");
  expect(status).toHaveTextContent("routingPage.seatAnswering");
  expect(status).toHaveTextContent("routingPage.resetIn");
});

it("prioritizes passed-over context before active and quota states", () => {
  show({
    row: {
      ...row,
      steps: [attempt(2), { kind: "context", candidate: 2, atMs: 1 }],
    },
  });
  const status = document.querySelector(".rt-seat-status");
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
      steps: [attempt(0)],
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

it.each(["responsive", "compact"] as const)(
  "distinguishes same-named providers in the %s stage without a catalog",
  (layout) => {
    const candidates = ["supplier-a", "supplier-b"].map((providerId) => ({
      providerId,
      providerName: "Synthetic relay",
      model: "shared-model",
    }));
    show({
      layout,
      row: { ...row, candidates, steps: [attempt(0), attempt(1)] },
    });
    const seats = document.querySelectorAll(".rt-accts li");
    expect(seats).toHaveLength(2);
    candidates.forEach((candidate, index) => {
      const label = `Synthetic relay · ${candidate.providerId}`;
      expect(seats[index].querySelector(".who")).toHaveTextContent(label);
      expect(seats[index].getAttribute("title")).toContain(label);
    });
  },
);

it("uses the public supplier ID when a configured name is duplicated outside the request", () => {
  show({
    row: {
      ...row,
      steps: [attempt(0)],
      candidates: [
        {
          providerId: "internal-execution-owner",
          sourceProviderId: "supplier-a",
          providerName: "Synthetic relay",
          model: "model-a",
          account: {
            ...row.candidates[0].account!,
            label: "Synthetic key label",
          },
        },
      ],
    },
    upstreams: ["supplier-a", "supplier-b"].map((id, index) => ({
      schemaVersion: 1,
      id,
      name: index ? " SYNTHETIC RELAY " : "Synthetic relay",
      revision: 1,
      hasApiKey: true,
      source: {
        kind: "api",
        baseUrl: "https://fixture.invalid/v1",
        apiFormat: "openai_chat",
        mode: "picked",
        models: [],
        catalog: [],
        groupsOnly: false,
      },
    })),
  });
  const seat = document.querySelector(".rt-accts li")!;
  expect(seat.querySelector(".who")).toHaveTextContent(
    "Synthetic relay · supplier-a",
  );
  expect(seat.getAttribute("title")).toContain("Synthetic relay · supplier-a");
  expect(seat.outerHTML).not.toContain("internal-execution-owner");
  expect(seat).not.toHaveTextContent("Synthetic key label");
});

it("does not treat one supplier's models, effort variants or execution identities as duplicate providers", () => {
  show({
    row: {
      ...allSeatsRow,
      candidates: ["model-a", "model-b", "model-b"].map((model, index) => ({
        providerId: `execution-owner-${index}`,
        sourceProviderId: "supplier-a",
        providerName: "Synthetic relay",
        model,
        fixedEffort: index === 2 ? "high" : undefined,
      })),
    },
    upstreams: [
      {
        schemaVersion: 1,
        id: "supplier-a",
        name: "Synthetic relay",
        revision: 1,
        hasApiKey: false,
        source: { kind: "codex" },
      },
    ],
  });
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(3);
  for (const label of document.querySelectorAll(".rt-accts .who")) {
    expect(label.textContent).toBe("Synthetic relay");
  }
});

it("distinguishes same-named providers across concurrent requests", () => {
  const older: RoutingTrace = {
    ...row,
    steps: [attempt(0)],
    candidates: [
      {
        providerId: "supplier-a",
        providerName: "Synthetic relay",
        model: "model-a",
      },
    ],
  };
  const latest: RoutingTrace = {
    ...older,
    id: "newer-request",
    candidates: [
      {
        providerId: "supplier-b",
        providerName: "Synthetic relay",
        model: "model-a",
      },
    ],
  };
  show({ row: latest, concurrent: [older, latest], animate: true });
  const labels = [...document.querySelectorAll(".rt-accts .who")].map(
    (label) => label.textContent,
  );
  expect(labels).toHaveLength(2);
  expect(labels).toEqual(
    expect.arrayContaining([
      "Synthetic relay · supplier-a",
      "Synthetic relay · supplier-b",
    ]),
  );
});

it("preserves configured account and key labels when their providers have duplicate names", () => {
  show({
    row: {
      ...allSeatsRow,
      candidates: row.candidates.map((candidate, index) => ({
        ...candidate,
        providerName: "Synthetic relay",
        account: { ...candidate.account!, label: `Synthetic seat ${index}` },
      })),
    },
  });
  expect(document.querySelectorAll(".rt-accts .who")).toHaveLength(3);
  for (const [index, label] of [
    ...document.querySelectorAll(".rt-accts .who"),
  ].entries()) {
    expect(label.textContent).toBe(`Synthetic seat ${index}`);
  }
});
