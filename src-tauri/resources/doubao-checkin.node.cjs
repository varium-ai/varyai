const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

async function run(
  input,
  {
    checked = false,
    switchDuringStatus = false,
    active = true,
    detailResponse,
    detailsFail = false,
    switchDuringDetails = false,
    detailsNeverResolve = false,
  } = {},
) {
  let current = "account-a";
  const switches = [];
  const calls = { status: 0, claim: 0 };
  let detailCalls = 0;
  const store = {
    getState: () => ({
      userInfo: { user_id_str: current, nickname: "Account A" },
    }),
  };
  const modules = {
    user: { e: store },
    profiles: {
      k: {
        getState: () => ({
          multiUserProfiles: [
            {
              entity_id: "account-a",
              nickname: "Account A",
              sec_id: "synthetic-sec-a",
            },
            {
              entity_id: "account-b",
              nickname: "Account B",
              sec_id: "synthetic-sec-b",
            },
          ],
        }),
      },
    },
    launch: {
      Z_: {
        getState: () => ({
          domains: {
            basic: {
              data: {
                ug_campaign_info: { campaign_group_code: "synthetic-campaign" },
              },
            },
          },
        }),
      },
    },
    api: {
      ugCampaignApiService: {
        async GetLandingPage({ scene }) {
          assert.equal(scene, "synthetic-campaign");
          calls.status++;
          if (switchDuringStatus) current = "account-b";
          return {
            code: 0,
            data: active
              ? {
                  chosen_type: checked ? 2 : 1,
                  main_page_info: { buttons: [{ button_type: 2 }] },
                }
              : null,
          };
        },
        async DailyCheckIn() {
          calls.claim++;
          return {
            code: 0,
            data: {
              checked_in: true,
              private_message: "synthetic-private-token",
            },
          };
        },
        async GetCampaignV3Detail({ campaign_group_code }) {
          assert.equal(campaign_group_code, "synthetic-campaign");
          detailCalls++;
          if (switchDuringDetails) current = "account-b";
          if (detailsFail) throw new Error("synthetic-private-detail");
          if (detailsNeverResolve) return new Promise(() => {});
          return (
            detailResponse ?? {
              code: 0,
              data: {
                tasks: [
                  {
                    task_kind: "daily_signin",
                    daily_checkin: {
                      checked_in_today: checked || calls.claim > 0,
                      continuous_days: 4,
                      total_days: 9,
                      reward: {
                        reward_amount: 200,
                        reward_type: "quota_grant",
                      },
                    },
                  },
                ],
              },
            }
          );
        },
      },
    },
  };
  modules.switcher = {
    switch: async (profile) => {
      switches.push(profile.entity_id);
      current = profile.entity_id;
      return { success: true };
    },
  };
  const requireMock = (id) => modules[id];
  requireMock.m = {
    user: function () {
      return Symbol("flow-account-user-info-store");
    },
    profiles: function () {
      return Symbol("multi-user-profile-store");
    },
    launch: function () {
      return { setDomainSuccess: () => {} }.setDomainSuccess("core");
    },
    api: function () {
      return "ugCampaignApiService:";
    },
    switcher: function () {
      return ["sec_to_user_id", "onSwitchSuccess"];
    },
  };
  let captures = 0;
  const chunks = [];
  chunks.push = (chunk) => {
    captures++;
    chunk[2](requireMock);
    return 1;
  };
  const context = vm.createContext({
    "@flow-web/desktop:stable": chunks,
    setTimeout,
    clearTimeout,
  });
  vm.runInContext(
    fs.readFileSync(path.join(__dirname, "doubao-checkin.js"), "utf8"),
    context,
  );
  const result = Array.isArray(input)
    ? await Promise.all(
        input.map((value) => context.varyaiDoubaoCheckin(value)),
      )
    : await context.varyaiDoubaoCheckin(input);
  return {
    result: JSON.parse(JSON.stringify(result)),
    calls,
    captures,
    switches,
    detailCalls,
  };
}

test("listing accounts does not query campaigns or claim rewards", async () => {
  const { result, calls } = await run({ mode: "accounts" });
  assert.equal(result.accounts.length, 2);
  assert.equal(result.currentAccountId, "account-a");
  assert.deepEqual(calls, { status: 0, claim: 0 });
});

test("repeated polling reuses one SDK capture without accumulating synthetic chunks", async () => {
  const { result, captures } = await run([
    { mode: "accounts" },
    { mode: "accounts" },
  ]);
  assert.equal(result.length, 2);
  assert.equal(captures, 1);
});

test("a selected non-current account is never replaced with the current account", async () => {
  const { result, calls } = await run({
    mode: "checkin",
    expectedAccountId: "account-b",
  });
  assert.equal(result.outcome, "switch_required");
  assert.deepEqual(calls, { status: 0, claim: 0 });
});

test("only an explicit switch action calls the official account switcher", async () => {
  const { result, calls, switches } = await run({
    mode: "switch",
    expectedAccountId: "account-a",
    targetAccountId: "account-b",
  });
  assert.equal(result.outcome, "switched");
  assert.deepEqual(switches, ["account-b"]);
  assert.deepEqual(calls, { status: 0, claim: 0 });
  const stale = await run({
    mode: "switch",
    expectedAccountId: "account-b",
    targetAccountId: "account-a",
  });
  assert.equal(stale.result.outcome, "switch_required");
  assert.deepEqual(stale.switches, []);
});

test("status is read-only and a confirmed check-in does not leak vendor fields", async () => {
  const read = await run({ mode: "status", expectedAccountId: "account-a" });
  assert.equal(read.result.outcome, "not_checked");
  assert.deepEqual(read.calls, { status: 1, claim: 0 });
  const claim = await run({ mode: "checkin", expectedAccountId: "account-a" });
  assert.equal(claim.result.outcome, "claimed");
  assert.deepEqual(claim.calls, { status: 1, claim: 1 });
  assert(!JSON.stringify(claim.result).includes("synthetic-private-token"));
});

test("an existing check-in or inactive campaign never sends a claim", async () => {
  for (const options of [{ checked: true }, { active: false }]) {
    const { result, calls } = await run(
      { mode: "checkin", expectedAccountId: "account-a" },
      options,
    );
    assert.equal(
      result.outcome,
      options.checked ? "already_checked" : "inactive",
    );
    assert.equal(calls.claim, 0);
  }
});

test("switching accounts during the status request prevents the claim", async () => {
  const { result, calls } = await run(
    { mode: "checkin", expectedAccountId: "account-a" },
    { switchDuringStatus: true },
  );
  assert.equal(result.outcome, "switch_required");
  assert.equal(calls.claim, 0);
});

test("official reward, consecutive days and total days are returned for a confirmed check-in", async () => {
  for (const input of ["status", "checkin"]) {
    const { result, detailCalls } = await run(
      { mode: input, expectedAccountId: "account-a" },
      { checked: input === "status" },
    );
    assert.equal(result.credit, 200);
    assert.equal(result.streak, 4);
    assert.equal(result.totalDays, 9);
    assert.equal(detailCalls, 1);
  }
});

test("missing or delayed campaign details do not turn a confirmed reward into a failure", async () => {
  for (const options of [
    { detailsFail: true },
    { detailsNeverResolve: true },
    { detailResponse: { code: 500 } },
    { detailResponse: { code: 0, data: { tasks: [] } } },
  ]) {
    const { result, calls } = await run(
      { mode: "checkin", expectedAccountId: "account-a" },
      options,
    );
    assert.equal(result.outcome, "claimed");
    assert.equal(result.credit, null);
    assert.equal(result.streak, null);
    assert.equal(result.totalDays, null);
    assert.equal(calls.claim, 1);
    assert(!JSON.stringify(result).includes("synthetic-private"));
  }
});

test("unconfirmed, ambiguous and invalid reward details never invent amounts or day counts", async () => {
  const daily = {
    checked_in_today: true,
    continuous_days: 4,
    total_days: 9,
    reward: { reward_amount: 200, reward_type: "quota_grant" },
  };
  for (const data of [
    {
      tasks: [
        {
          task_kind: "daily_signin",
          daily_checkin: { ...daily, checked_in_today: false },
        },
      ],
    },
    {
      tasks: [daily, daily].map((daily_checkin) => ({
        task_kind: "daily_signin",
        daily_checkin,
      })),
    },
    {
      tasks: [
        {
          task_kind: "daily_signin",
          daily_checkin: {
            ...daily,
            continuous_days: -1,
            total_days: 2.5,
            reward: { reward_amount: -10, reward_type: "quota_grant" },
          },
        },
      ],
    },
  ]) {
    const { result } = await run(
      { mode: "status", expectedAccountId: "account-a" },
      { checked: true, detailResponse: { code: 0, data } },
    );
    assert.equal(result.credit, null);
    assert.equal(result.streak, null);
    assert.equal(result.totalDays, null);
  }
  const otherReward = await run(
    { mode: "status", expectedAccountId: "account-a" },
    {
      checked: true,
      detailResponse: {
        code: 0,
        data: {
          tasks: [
            {
              task_kind: "daily_signin",
              daily_checkin: {
                ...daily,
                reward: { reward_amount: 200, reward_type: "other_reward" },
              },
            },
          ],
        },
      },
    },
  );
  assert.equal(otherReward.result.credit, null);
  assert.equal(otherReward.result.streak, 4);
});

test("an account change during the details request never attributes another account's reward", async () => {
  const { result } = await run(
    { mode: "status", expectedAccountId: "account-a" },
    { checked: true, switchDuringDetails: true },
  );
  assert.equal(result.outcome, "switch_required");
  assert.equal(result.credit, undefined);
});
