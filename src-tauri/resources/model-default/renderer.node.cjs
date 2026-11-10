"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { applyDefault } = require("./renderer.cjs");
function fixture(app, initial = {}) {
  const values = new Map(Object.entries(initial));
  const storage = {
    get length() {
      return values.size;
    },
    key: (index) => [...values.keys()][index] ?? null,
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
  return {
    values,
    scope: { localStorage: storage },
    plan: { app, revision: "one", model: "Synthetic model", restore: false },
  };
}
test("WorkBuddy changes only general new-task defaults and seeds new account keys", () => {
  const base = "cb-newtask:model",
    account = `${base}:synthetic-account`;
  const prior = JSON.stringify({
    id: "native",
    isThinking: true,
    reasoningEffort: "high",
  });
  const f = fixture("workbuddy-ai", {
    [account]: prior,
    [`${account}:buddy-app`]: prior,
    credential: "synthetic-secret",
  });
  assert.equal(applyDefault(f.scope, f.plan).applied, true);
  assert.deepEqual(JSON.parse(f.scope.localStorage.getItem(account)), {
    id: "custom-local:Synthetic model",
    isThinking: false,
  });
  assert.equal(f.values.get(`${account}:buddy-app`), prior);
  assert.equal(f.values.get("credential"), "synthetic-secret");
  assert.equal(
    JSON.parse(f.scope.localStorage.getItem(`${base}:new-account`)).id,
    "custom-local:Synthetic model",
  );
  f.scope.localStorage.setItem(account, prior);
  assert.equal(f.scope.localStorage.getItem(account), prior);
  applyDefault(f.scope, { ...f.plan, restore: true });
  assert.equal(f.values.get(account), prior);
  assert.equal(f.values.has(`${base}:new-account`), false);
});
test("Qoder uses the verified native BYOK key and leaves remote runtime/session choices alone", () => {
  const base = "qoder:chat-composer:selected-model",
    account = `${base}:account:synthetic`;
  const f = fixture("qoder", {
    [account]: '"auto"',
    [`${account}:runtime:ssh`]: '"remote"',
    sessionModel: '"existing"',
  });
  const plan = { ...f.plan, nativeKey: "byok:synthetic-varyai" };
  assert.equal(applyDefault(f.scope, plan).applied, true);
  assert.equal(f.values.get(account), '"byok:synthetic-varyai"');
  assert.equal(f.values.get(`${account}:runtime:ssh`), '"remote"');
  assert.equal(f.values.get("sessionModel"), '"existing"');
  applyDefault(f.scope, { ...plan, restore: true });
  assert.equal(f.values.get(account), '"auto"');
});
test("restores only values still owned by this bridge, including after a renderer restart", () => {
  const f = fixture("workbuddy", {
    "cb-newtask:model": '{"id":"native","isThinking":false}',
  });
  applyDefault(f.scope, f.plan);
  delete f.scope.__varyaiModelDefault;
  applyDefault(f.scope, { ...f.plan, restore: true });
  assert.equal(
    f.values.get("cb-newtask:model"),
    '{"id":"native","isThinking":false}',
  );
  assert.equal(
    [...f.values.keys()].some((key) => key.startsWith("__varyai")),
    false,
  );
});

test("a later explicit default retains intervening native choices as its recovery baseline", () => {
  const key = "cb-newtask:model:synthetic";
  const f = fixture("workbuddy", {
    [key]: '{"id":"original","isThinking":false}',
  });
  applyDefault(f.scope, f.plan);
  const manual =
    '{"id":"native-choice","isThinking":true,"reasoningEffort":"high"}';
  f.scope.localStorage.setItem(key, manual);
  const next = { ...f.plan, model: "second", revision: "two" };
  applyDefault(f.scope, next);
  assert.equal(JSON.parse(f.values.get(key)).id, "custom-local:second");
  applyDefault(f.scope, { ...next, restore: true });
  assert.equal(f.values.get(key), manual);
});

test("the shared Storage prototype wrapper affects only this renderer's local model keys", () => {
  class Storage {
    constructor() {
      this.values = new Map();
    }
    get length() {
      return this.values.size;
    }
    key(i) {
      return [...this.values.keys()][i] ?? null;
    }
    getItem(key) {
      return this.values.get(key) ?? null;
    }
    setItem(key, value) {
      this.values.set(key, value);
    }
    removeItem(key) {
      this.values.delete(key);
    }
  }
  const original = Storage.prototype.getItem,
    local = new Storage(),
    session = new Storage();
  const scope = { localStorage: local, Storage };
  const plan = fixture("workbuddy").plan;
  applyDefault(scope, plan);
  assert.equal(session.getItem("cb-newtask:model:synthetic"), null);
  assert.equal(
    JSON.parse(local.getItem("cb-newtask:model:synthetic")).id,
    `custom-local:${plan.model}`,
  );
  applyDefault(scope, { ...plan, restore: true });
  assert.equal(Storage.prototype.getItem, original);
});
test("rejects unsafe model IDs, unknown clients and malformed baselines without changing storage", () => {
  for (const plan of [
    { app: "browser" },
    { model: "https://synthetic.invalid/?secret=synthetic" },
    { nativeKey: "auto", app: "qoder" },
  ]) {
    const f = fixture("workbuddy");
    assert.throws(() => applyDefault(f.scope, { ...f.plan, ...plan }));
    assert.equal(f.values.size, 0);
  }
  const f = fixture("workbuddy", { "cb-newtask:model": "synthetic-malformed" });
  assert.throws(() => applyDefault(f.scope, f.plan));
  assert.equal(f.values.get("cb-newtask:model"), "synthetic-malformed");
});
