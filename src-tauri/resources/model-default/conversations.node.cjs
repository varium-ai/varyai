"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { applyConversationDefault } = require("./conversations.cjs");

function fixture(app) {
  const sessions = Array.from({ length: 103 }, (_, index) => ({
    id: `synthetic-${index}`,
    transport: "local",
    model: "custom-local:previous",
    messages: [{ text: "Synthetic retained history" }],
    thoughtLevel: "high",
  }));
  sessions.push({
    id: "synthetic-cloud",
    transport: "cloud",
    model: "official",
  });
  const calls = [];
  const scope = {
    async __wbInvoke(channel, subject, ...args) {
      assert.equal(subject, undefined);
      calls.push({ channel, args });
      if (channel === "wb:conversations:list") {
        const { page, size, transport } = args[0];
        assert.equal(transport, "local");
        return {
          items: sessions.slice((page - 1) * size, page * size),
          total: sessions.length,
        };
      }
      const session = sessions.find((session) => session.id === args[0]);
      assert.ok(session);
      if (channel === "wb:conversations:configSetModel") {
        session.model = args[1];
        return undefined;
      }
      assert.equal(channel, "wb:conversations:get");
      return {
        info: { id: session.id, transport: session.transport },
        configManager: { model: session.model },
      };
    },
  };
  return {
    scope,
    sessions,
    calls,
    plan: { app, model: "next", revision: "one", restore: false },
  };
}

for (const app of ["workbuddy", "workbuddy-ai"]) {
  test(`${app} updates existing paginated sessions through native configuration and preserves history`, async () => {
    const f = fixture(app);
    const history = structuredClone(f.sessions);
    assert.deepEqual(await applyConversationDefault(f.scope, f.plan), {
      applied: true,
    });
    for (const [index, session] of f.sessions.entries()) {
      assert.deepEqual(session, {
        ...history[index],
        model: session.transport === "local" ? "custom-local:next" : "official",
      });
    }
    assert.equal(
      f.calls.filter(
        (call) => call.channel === "wb:conversations:configSetModel",
      ).length,
      103,
    );
    const lastList = f.calls.findLastIndex(
      (call) => call.channel === "wb:conversations:list",
    );
    const firstWrite = f.calls.findIndex(
      (call) => call.channel === "wb:conversations:configSetModel",
    );
    assert.ok(
      lastList < firstWrite,
      "snapshot IDs before writes can reorder the native list",
    );
    const count = f.calls.length;
    await applyConversationDefault(f.scope, f.plan);
    assert.equal(
      f.calls.length,
      count,
      "polling must not overwrite later manual choices",
    );
    await applyConversationDefault(f.scope, {
      ...f.plan,
      model: "third",
      revision: "two",
    });
    assert.equal(f.sessions[0].model, "custom-local:third");
  });
}

test("Qoder and restoration do not change existing WorkBuddy conversations", async () => {
  const f = fixture("qoder");
  const before = structuredClone(f.sessions);
  await applyConversationDefault(f.scope, f.plan);
  await applyConversationDefault(f.scope, {
    ...f.plan,
    app: "workbuddy",
    restore: true,
  });
  assert.deepEqual(f.sessions, before);
  assert.equal(f.calls.length, 0);
});

test("native refusal and failed readback never claim that the default reached old sessions", async () => {
  for (const failure of ["refusal", "readback", "partial"]) {
    const f = fixture("workbuddy");
    const invoke = f.scope.__wbInvoke;
    f.scope.__wbInvoke = async (channel, subject, ...args) => {
      if (
        failure === "refusal" &&
        channel === "wb:conversations:configSetModel"
      )
        return { errorCode: "synthetic-refusal" };
      if (failure === "readback" && channel === "wb:conversations:get")
        return {
          info: { id: args[0], transport: "local" },
          configManager: { model: "previous" },
        };
      const result = await invoke(channel, subject, ...args);
      return failure === "partial" && channel === "wb:conversations:list"
        ? { ...result, partial: true }
        : result;
    };
    await assert.rejects(
      applyConversationDefault(f.scope, f.plan),
      /^Error: model_default_unavailable$/,
    );
    if (failure === "partial")
      assert.equal(f.sessions[0].model, "custom-local:previous");
  }
});

test("overlapping polls share native writes and a subsequent renderer launch reapplies the default", async () => {
  const f = fixture("workbuddy-ai");
  const invoke = f.scope.__wbInvoke;
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  let waiting = true;
  f.scope.__wbInvoke = async (...args) => {
    if (args[0] === "wb:conversations:configSetModel" && waiting) {
      waiting = false;
      await gate;
    }
    return invoke(...args);
  };
  const first = applyConversationDefault(f.scope, f.plan);
  await new Promise((resolve) => setImmediate(resolve));
  const second = applyConversationDefault(f.scope, f.plan);
  release();
  await Promise.all([first, second]);
  assert.equal(
    f.calls.filter((call) => call.channel === "wb:conversations:configSetModel")
      .length,
    103,
  );
  f.sessions[0].model = "manual-selection";
  await applyConversationDefault(f.scope, f.plan);
  assert.equal(f.sessions[0].model, "manual-selection");
  delete f.scope.__varyaiWorkBuddyConversationDefault;
  await applyConversationDefault(f.scope, f.plan);
  assert.equal(f.sessions[0].model, "custom-local:next");
});

test("incomplete, oversized and repeated native list pages never start model writes", async () => {
  for (const result of [
    { items: [], total: 101 },
    { items: [], total: 10001 },
    {
      items: [
        { id: "same", transport: "local" },
        { id: "same", transport: "local" },
      ],
      total: 2,
    },
    { items: [{ id: "unknown", transport: "unrecognized" }], total: 1 },
  ]) {
    const f = fixture("workbuddy");
    f.scope.__wbInvoke = async (channel) => {
      assert.equal(channel, "wb:conversations:list");
      return result;
    };
    await assert.rejects(
      applyConversationDefault(f.scope, f.plan),
      /^Error: model_default_unavailable$/,
    );
    assert.equal(f.sessions[0].model, "custom-local:previous");
  }
});
