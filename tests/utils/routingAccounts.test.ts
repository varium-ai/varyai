import { expect, it } from "vitest";
import { foldRoutingAccounts } from "@/lib/routingAccounts";
import type { RoutingAccount, RoutingTrace } from "@/lib/api/routingTrace";

function account(
  id = "key-a",
  model = "model-a",
  kind = "key",
): RoutingAccount {
  return {
    id,
    kind,
    providerId: "vendor",
    providerName: "Synthetic vendor",
    model,
    restUntil: 0,
    windows: [],
  } as RoutingAccount;
}
function route(
  id: string,
  accounts: RoutingAccount[],
  steps: RoutingTrace["steps"] = [],
): RoutingTrace {
  return {
    id,
    startedAt: Number(id),
    agent: "codex",
    model: "group/fixture",
    groupName: "Fixture",
    strategy: "ordered",
    state: "completed",
    durationMs: 100,
    candidates: accounts.map((a) => ({
      providerId: a.providerId,
      providerName: a.providerName,
      model: a.model,
      account: a,
    })),
    steps,
  };
}

it("only includes observed candidates, retaining untried standby routes and omitting catalog-only rows", () => {
  const a = account();
  const standby = account("key-b");
  const result = foldRoutingAccounts(
    [route("1", [a, standby])],
    [account("unused")],
    true,
  );
  expect(result.map((row) => row.id)).toEqual(["key-a", "key-b"]);
  expect(
    result.every((row) => row.tried === 0 && row.models.length === 0),
  ).toBe(true);
  expect(foldRoutingAccounts([], [a], true)).toEqual([]);
});

it("merges models by credential identity and counts attempts, answers, and failures over the listed requests", () => {
  const result = foldRoutingAccounts(
    [
      route(
        "1",
        [account()],
        [
          { kind: "attempt", candidate: 0, atMs: 0 },
          { kind: "rejected", candidate: 0, atMs: 2, status: 429 },
          { kind: "attempt", candidate: 0, atMs: 3 },
          { kind: "completed", candidate: 0, atMs: 100, status: 200 },
        ],
      ),
      route(
        "2",
        [account("key-a", "model-b")],
        [
          { kind: "attempt", candidate: 0, atMs: 0 },
          { kind: "completed", candidate: 0, atMs: 100, status: 200 },
        ],
      ),
    ],
    [],
    false,
  );
  expect(result).toHaveLength(1);
  expect(result[0]).toMatchObject({
    tried: 3,
    answered: 2,
    failed: 1,
    models: ["model-a", "model-b"],
    lastAnswered: 102,
  });
});

it("keeps credential rotations and different keys separate even when their display names match", () => {
  const result = foldRoutingAccounts(
    [route("1", [account("old"), account("new")])],
    [],
    true,
  );
  expect(result).toHaveLength(2);
  expect(new Set(result.map((row) => row.id)).size).toBe(2);
});

it("matches live telemetry by exact identity and model and never applies it to historical views", () => {
  const recorded = {
    ...account(),
    restUntil: 10,
    windows: [{ usedPercent: 25, windowSeconds: 18000, resetsAt: 100 }],
  };
  const current = {
    ...account(),
    restUntil: 20,
    windows: [{ usedPercent: 75, windowSeconds: 18000, resetsAt: 200 }],
  };
  expect(
    foldRoutingAccounts([route("1", [recorded])], [current], true)[0]
      .observations[0].restUntil,
  ).toBe(20);
  expect(
    foldRoutingAccounts([route("1", [recorded])], [current], false)[0]
      .observations[0].restUntil,
  ).toBe(10);
  expect(
    foldRoutingAccounts(
      [route("1", [recorded])],
      [{ ...current, id: "rotated" }],
      true,
    )[0].observations[0].restUntil,
  ).toBe(10);
});

it("does not turn a model-specific cooldown into an account-wide rest or count cancelled attempts as failures", () => {
  const result = foldRoutingAccounts(
    [
      route(
        "1",
        [{ ...account(), restUntil: 100 }, account("key-a", "model-b")],
        [
          { kind: "attempt", candidate: 0, atMs: 0 },
          { kind: "cancelled", candidate: 0, atMs: 2, status: 499 },
        ],
      ),
    ],
    [],
    true,
  );
  expect(result[0]).toMatchObject({ tried: 0, failed: 0, answered: 0 });
  expect(result[0].observations.map((row) => row.restUntil)).toEqual([100, 0]);
});

it("retains provider/model summaries without borrowing credential identities or today's telemetry", () => {
  const archive = { ...route("1", [account()]), archived: true, status: 200 };
  const result = foldRoutingAccounts([archive], [account()], true);
  expect(result).toHaveLength(1);
  expect(result[0]).toMatchObject({
    kind: "provider",
    summaryOnly: true,
    requests: 1,
    tried: 0,
    answered: 1,
    failed: 0,
    models: ["model-a"],
  });
  expect(result[0].id).not.toBe("key-a");
  expect(result[0].observations).toEqual([
    {
      providerId: "vendor",
      providerName: "Synthetic vendor",
      model: "model-a",
      restUntil: 0,
      windows: [],
    },
  ]);
});

it("aggregates archived outcomes by provider/model without inferring fallback attempts or merging with live keys", () => {
  const result = foldRoutingAccounts(
    [
      { ...route("1", [account()]), archived: true, status: 200 },
      {
        ...route("2", [account()]),
        archived: true,
        state: "failed",
        status: 429,
      },
      {
        ...route("3", [account()]),
        archived: true,
        state: "cancelled",
        status: 499,
      },
      route("4", [account()]),
    ],
    [account()],
    true,
  );
  expect(result).toHaveLength(2);
  const summary = result.find((row) => row.summaryOnly);
  expect(summary).toMatchObject({
    requests: 3,
    tried: 0,
    answered: 1,
    failed: 1,
    failures: { "429": 1 },
    lastAnswered: 101,
  });
  expect(result.find((row) => !row.summaryOnly)).toMatchObject({
    id: "key-a",
    tried: 0,
  });
});

it("keeps different archived models and providers separate and skips summaries with no unambiguous destination", () => {
  const result = foldRoutingAccounts(
    [
      { ...route("1", [account()]), archived: true },
      { ...route("2", [account("key-a", "model-b")]), archived: true },
      {
        ...route("3", [{ ...account(), providerId: "other" }]),
        archived: true,
      },
      { ...route("4", []), archived: true },
      { ...route("5", [account(), account("other-key")]), archived: true },
      { ...route("6", [{ ...account(), providerId: "" }]), archived: true },
    ],
    [],
    false,
  );
  expect(result).toHaveLength(3);
  expect(result.every((row) => row.requests === 1 && row.summaryOnly)).toBe(
    true,
  );
  expect(
    result.map((row) => [row.providerId, row.observations[0].model]),
  ).toEqual([
    ["other", "model-a"],
    ["vendor", "model-a"],
    ["vendor", "model-b"],
  ]);
});

it("retains bounded failure categories and counts stream failure once after connection", () => {
  const result = foldRoutingAccounts(
    [
      route(
        "1",
        [account()],
        [
          { kind: "attempt", candidate: 0, atMs: 0 },
          { kind: "connected", candidate: 0, atMs: 1, status: 200 },
          { kind: "failed", candidate: 0, atMs: 2, status: 502 },
        ],
      ),
    ],
    [],
    false,
  );
  expect(result[0]).toMatchObject({
    tried: 1,
    failed: 1,
    answered: 0,
    models: [],
  });
});
