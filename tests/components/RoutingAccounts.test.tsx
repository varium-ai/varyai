import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it } from "vitest";
import { RoutingAccounts } from "@/components/routing/RoutingAccounts";
import type { RoutingAccount, RoutingTrace } from "@/lib/api/routingTrace";

function fixtures(count = 1, kind: "account" | "key" = "key") {
  const accounts: RoutingAccount[] = Array.from(
    { length: count },
    (_, index) => ({
      id: `synthetic-key-${index}`,
      kind,
      providerId: "fixture",
      providerName: "Synthetic provider",
      model: "model-a",
      restUntil: 0,
      windows: [],
    }),
  );
  const request: RoutingTrace = {
    id: "fixture-request",
    startedAt: Date.now(),
    agent: "codex",
    model: "fixture",
    groupName: "",
    strategy: "ordered",
    state: "completed",
    durationMs: 100,
    candidates: accounts.map((account) => ({ ...account, account })),
    steps: [
      { kind: "attempt", candidate: 0, atMs: 0 },
      { kind: "completed", candidate: 0, atMs: 100, status: 200 },
    ],
  };
  return { accounts, request };
}
function show(count = 1, kind: "account" | "key" = "key", live = true) {
  const { accounts, request } = fixtures(count, kind);
  return render(
    <RoutingAccounts
      requests={[request]}
      current={accounts}
      live={live}
      refreshing={false}
      refresh={() => {}}
      accountName={(candidate) => candidate.providerName}
    />,
  );
}

beforeEach(() => localStorage.clear());

it("omits catalog-only routes and does not show quota placeholders, verified status, or a refresh button for API keys", () => {
  const { container } = show();
  const panel = screen.getByRole("region", { name: "routingPage.accounts" });
  expect(within(panel).getByText("Synthetic provider")).toBeInTheDocument();
  expect(container.querySelectorAll("[data-account-row]")).toHaveLength(1);
  expect(screen.queryByText("routingPage.quotaUnknown")).toBeNull();
  expect(screen.queryByText("routingPage.available")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "routingPage.refreshAccounts" }),
  ).toBeNull();
  expect(container.querySelector(".dot")).toBeNull();
});

it("starts three keys folded and preserves an explicit expansion across remounts", async () => {
  const user = userEvent.setup();
  const view = show(3);
  const fold = screen.getByRole("button", { name: /routingPage.keyCount/ });
  expect(fold).toHaveAttribute("aria-expanded", "false");
  expect(view.container.querySelectorAll("[data-account-row]")).toHaveLength(0);
  await user.click(fold);
  expect(fold).toHaveAttribute("aria-expanded", "true");
  expect(view.container.querySelectorAll("[data-account-row]")).toHaveLength(3);
  view.unmount();
  const remount = show(3);
  expect(
    screen.getByRole("button", { name: /routingPage.keyCount/ }),
  ).toHaveAttribute("aria-expanded", "true");
  expect(remount.container.querySelectorAll("[data-account-row]")).toHaveLength(
    3,
  );
});

it("starts three to five subscription accounts expanded and six folded like the reference implementation", () => {
  const three = show(3, "account");
  expect(
    screen.getByRole("button", { name: /routingPage.accountCount/ }),
  ).toHaveAttribute("aria-expanded", "true");
  three.unmount();
  const six = show(6, "account");
  expect(
    screen.getByRole("button", { name: /routingPage.accountCount/ }),
  ).toHaveAttribute("aria-expanded", "false");
  expect(six.container.querySelectorAll("[data-account-row]")).toHaveLength(0);
});

it("only offers live subscription observations and suppresses today's quotas on historical requests", () => {
  const live = show(1, "account");
  expect(
    screen.getByRole("button", { name: "routingPage.refreshAccounts" }),
  ).toBeInTheDocument();
  expect(
    screen.getByText("routingPage.accountQuotaUnknown"),
  ).toBeInTheDocument();
  live.unmount();
  show(1, "account", false);
  expect(
    screen.queryByRole("button", { name: "routingPage.refreshAccounts" }),
  ).toBeNull();
  expect(screen.queryByText("routingPage.accountQuotaUnknown")).toBeNull();
});

it("reflects a filtered-empty request view instead of showing unrelated catalog telemetry", () => {
  const { accounts } = fixtures(5);
  const { container } = render(
    <RoutingAccounts
      requests={[]}
      current={accounts}
      live
      refreshing={false}
      refresh={() => {}}
      accountName={(candidate) => candidate.providerName}
    />,
  );
  expect(screen.getByText("routingPage.noAccounts")).toBeInTheDocument();
  expect(container.querySelectorAll("[data-account-row]")).toHaveLength(0);
});

it("masks historical provider names and excludes live telemetry without borrowing live quota data", () => {
  const { accounts, request } = fixtures(1, "account");
  const historical: RoutingTrace = JSON.parse(JSON.stringify(request));
  accounts[0].restUntil = Date.now() / 1000 + 300;
  accounts[0].windows = [
    {
      usedPercent: 80,
      windowSeconds: 18000,
      resetsAt: Date.now() / 1000 + 3600,
    },
  ];
  const { container } = render(
    <RoutingAccounts
      requests={[historical]}
      current={accounts}
      live={false}
      refreshing={false}
      refresh={() => {}}
      accountName={() => "Masked account"}
    />,
  );
  expect(screen.getByText("Masked account")).toBeInTheDocument();
  expect(screen.queryByText("Synthetic provider")).toBeNull();
  expect(screen.queryByText("routingPage.accountQuotaUnknown")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "routingPage.refreshAccounts" }),
  ).toBeNull();
  expect(container.querySelector(".rt-account-rest")).toBeNull();
  expect(container.querySelector('[role="progressbar"]')).toBeNull();
});

it("merges a key's models into one row and masks the provider heading", () => {
  const { request, accounts } = fixtures();
  const modelB = { ...accounts[0], model: "model-b" };
  const second = {
    ...request,
    id: "second",
    candidates: [
      {
        providerId: modelB.providerId,
        providerName: modelB.providerName,
        model: modelB.model,
        account: modelB,
      },
    ],
  };
  const { container } = render(
    <RoutingAccounts
      requests={[request, second]}
      current={[...accounts, modelB]}
      live
      refreshing={false}
      refresh={() => {}}
      accountName={() => "Masked account"}
    />,
  );
  expect(container.querySelectorAll("[data-account-row]")).toHaveLength(1);
  expect(screen.getByText("model-a")).toBeInTheDocument();
  expect(screen.getByText("model-b")).toBeInTheDocument();
  expect(screen.getByText("Masked account")).toBeInTheDocument();
  expect(screen.queryByText("Synthetic provider")).toBeNull();
});

it("keeps model cooldowns visible without showing subscription-quota placeholders for a key", () => {
  const { request, accounts } = fixtures();
  accounts[0].restUntil = Date.now() / 1000 + 300;
  const { container } = render(
    <RoutingAccounts
      requests={[request]}
      current={accounts}
      live
      refreshing={false}
      refresh={() => {}}
      accountName={(candidate) => candidate.providerName}
    />,
  );
  expect(container.querySelector(".rt-account-rest")).toHaveTextContent(
    "model-a",
  );
  expect(screen.queryByText("routingPage.accountQuotaUnknown")).toBeNull();
  expect(screen.queryByText("routingPage.available")).toBeNull();
});

it("does not count model-only cooldowns as resting accounts in a folded pool", () => {
  const { request, accounts } = fixtures(3);
  for (const account of accounts) account.restUntil = Date.now() / 1000 + 300;
  render(
    <RoutingAccounts
      requests={[request]}
      current={accounts}
      live
      refreshing={false}
      refresh={() => {}}
      accountName={(candidate) => candidate.providerName}
    />,
  );
  expect(screen.queryByText(/routingPage.accountsResting/)).toBeNull();
});
