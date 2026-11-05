"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { createTraeRewards } = require("./rewards.cjs");
const user = {
  userId: "synthetic-account",
  account: { username: "Synthetic account" },
  token: "synthetic-private-token",
};
function fixture({ checked = false, code = 0, change = false } = {}) {
  const calls = [];
  let claims = 0;
  return {
    calls,
    run: createTraeRewards({
      async invoke(channel) {
        calls.push(channel);
        if (channel.endsWith("getUserInfo"))
          return change && claims
            ? { ...user, userId: "synthetic-other" }
            : user;
        if (channel.endsWith("fetchCheckinCreditsStatus"))
          return {
            status: 200,
            data: { enable: true, checked_in: checked, credits: 100 },
          };
        claims++;
        return { status: 200, data: { code, credits: 123 } };
      },
    }),
  };
}
test("TRAE refresh exports only reward/account metadata and never claims", async () => {
  const f = fixture();
  const result = await f.run({ mode: "status" });
  assert.equal(result.outcome, "not_checked");
  assert.match(result.accountId, /^[a-f0-9]{64}$/);
  assert.ok(!JSON.stringify(result).includes(user.token));
  assert.ok(!f.calls.some((c) => c.endsWith("claimCheckinCredits")));
});
test("TRAE checks selected identity, already checked state and official claim codes", async () => {
  for (const code of [0, 9074, 9095, 9999]) {
    const f = fixture({ code });
    const status = await f.run({ mode: "status" });
    const result = await f.run({
      mode: "claim",
      expectedAccountId: status.accountId,
    });
    assert.equal(
      result.outcome,
      { 0: "claimed", 9074: "own_app_required", 9095: "ineligible" }[code] ??
        "failed",
    );
    if (code === 0) assert.equal(result.credit, 123);
  }
  const f = fixture({ checked: true });
  const s = await f.run({ mode: "status" });
  assert.equal(
    (await f.run({ mode: "claim", expectedAccountId: s.accountId })).outcome,
    "already_checked",
  );
  assert.ok(!f.calls.some((c) => c.endsWith("claimCheckinCredits")));
});
test("TRAE refuses stale selections and account changes without misattribution", async () => {
  const f = fixture();
  assert.equal(
    (await f.run({ mode: "claim", expectedAccountId: "synthetic-stale" }))
      .outcome,
    "switch_required",
  );
  assert.ok(!f.calls.some((c) => c.endsWith("claimCheckinCredits")));
  const changed = fixture({ change: true });
  const s = await changed.run({ mode: "status" });
  assert.equal(
    (await changed.run({ mode: "claim", expectedAccountId: s.accountId }))
      .outcome,
    "switch_required",
  );
});
test("TRAE inactive campaigns do not require an absent checked-in field", async () => {
  const run = createTraeRewards({
    async invoke(channel) {
      if (channel.endsWith("getUserInfo")) return user;
      assert.ok(channel.endsWith("fetchCheckinCreditsStatus"));
      return { status: 200, data: { enable: false } };
    },
  });
  assert.equal((await run({ mode: "status" })).outcome, "inactive");
});
test("TRAE confirms legacy successful responses through a status read instead of trusting HTTP 200", async () => {
  for (const confirmed of [true, false]) {
    let posted = false;
    const run = createTraeRewards({
      async invoke(channel) {
        if (channel.endsWith("getUserInfo")) return user;
        if (channel.endsWith("claimCheckinCredits")) {
          posted = true;
          return { status: 200, data: {} };
        }
        return {
          status: 200,
          data: { enable: true, checked_in: posted && confirmed },
        };
      },
    });
    const status = await run({ mode: "status" });
    assert.equal(
      (await run({ mode: "claim", expectedAccountId: status.accountId }))
        .outcome,
      confirmed ? "claimed" : "failed",
    );
  }
});
