"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { createStartingModelAdapter } = require("./starting-model.cjs");

function fixture(legacyModuleId) {
  const recent = {},
    calls = [],
    sessions = { existing: { modelId: "original" } };
  const rows = {
    agent: [{ custom_model_id: "native-a", name: "provider//model-a" }],
    solo_agent: [{ custom_model_id: "native-a", name: "provider//model-a" }],
  };
  const label = (type) => (["assistant", "solo_agent"].includes(type) ? type : "agent");
  const domain = {
    setNewSessionSelection: (type, selection) =>
      calls.push(["clear", type, selection]),
    setRecentUserSelection: (type, selection) => {
      recent[type] = selection;
    },
    getRecentUserSelectionMap: () => recent,
    getUserSelectedModel: (session, type) => recent[label(type)],
  };
  const port = {
    getHostContext: () => ({ modelSelectionPolicy: {} }),
    persistUserModelSelection: async (value) => {
      calls.push(["persist", value]);
      return true;
    },
  };
  const services = {
    ModelStore: {
      modelsByAgentType: { get: (type) => rows[label(type)] ?? [] },
    },
    ModelDomainService: domain,
    ModelPersistenceService: {
      saveRecentUserSelection: async () => calls.push(["save"]),
    },
    ModelAppService: {
      notifyHostModelSelectionChange: (value) => calls.push(["notify", value]),
    },
  };
  const modules = {
    14284: {
      $t: {
        getInstance: () => ({
          isInitialized: () => true,
          resolve: (id) => services[Symbol.keyFor(id)],
        }),
      },
    },
    8594: { XT: { tryResolve: () => port } },
    58660: {
      NR: (row, type) => `${type}:${row.custom_model_id}`,
      VY: { Manual: 0 },
      UU: (row) => row.selectable !== false,
    },
    87327: { qR: label },
    46623: { KP: (type) => type },
    73916: {
      Eo: {
        Chat: "chat",
        Builder: "builder",
        Assistant: "assistant",
        SoloAgent: "solo_agent",
        Unsupported: "unsupported",
      },
    },
  };
  const req = (id) => modules[id];
  if (legacyModuleId !== undefined) {
    const mapping = modules[46623];
    delete modules[46623];
    modules[legacyModuleId] = mapping;
    req.m = {
      [legacyModuleId]: function () {
        return { KP: "dev_agent", other: "builder_with_mcp" };
      },
      unrelated() {
        throw Error("must not load");
      },
    };
  }
  return {
    adapter: createStartingModelAdapter(req),
    calls,
    recent,
    rows,
    port,
    services,
    sessions,
  };
}

test("sets and verifies new-session selections once per supported native Agent label", async () => {
  const f = fixture();
  await f.adapter.select("native-a");
  assert.deepEqual(Object.keys(f.recent), ["agent", "solo_agent"]);
  assert.equal(f.calls.filter(([op]) => op === "notify").length, 2);
  for (const [op, value] of f.calls) {
    if (op === "persist" || op === "notify")
      assert.equal(value.sessionId, undefined);
    if (op === "persist") assert.equal(value.selection.mode, 0);
  }
  assert.deepEqual(f.sessions, { existing: { modelId: "original" } });
});

test("skips forced Auto and acknowledges the default for each native selection scope", async () => {
  const f = fixture();
  f.rows.assistant = [...f.rows.agent];
  f.port.getHostContext = () => ({
    modelSelectionPolicy: {
      strategy: "ai-chat",
      agentTypeAliases: { assistant: "fixed_assistant" },
      forcedModeByAgentType: { fixed_assistant: 1 },
    },
  });
  let scope = "first-workspace";
  f.port.getSelectionStorageIdentity = () => scope;
  assert.equal(f.adapter.needsSelection("native-a"), true);
  await f.adapter.select("native-a");
  assert.deepEqual(Object.keys(f.recent), ["agent", "solo_agent"]);
  assert.equal(f.adapter.needsSelection("native-a"), false);
  assert.equal(f.adapter.needsSelection("native-b"), true);
  scope = "another-workspace";
  assert.equal(f.adapter.needsSelection("native-a"), true);
});

test("a failed label stays pending while other labels receive the default", async () => {
  const f = fixture();
  f.port.persistUserModelSelection = async ({ agentType }) => agentType !== "chat";
  await assert.rejects(f.adapter.select("native-a"), /starting_model_unavailable/);
  assert.equal(f.recent.agent, undefined);
  assert.equal(f.recent.solo_agent.modelId, "solo_agent:native-a");
  assert.equal(f.adapter.needsSelection("native-a"), true);
});

test("waits for the native catalog and saved selection before writing the startup model", async () => {
  const f = fixture();
  let loaded = false;
  let restored = false;
  f.port.modelHostService = {
    ensureModelsLoaded: async () => loaded,
    isModelSelectionReady: () => restored,
  };

  await assert.rejects(
    f.adapter.select("native-a"),
    /starting_model_unavailable/,
  );
  loaded = true;
  await assert.rejects(
    f.adapter.select("native-a"),
    /starting_model_unavailable/,
  );
  assert.deepEqual(f.calls, []);

  f.recent.agent = { modelId: "previous", mode: 0 };
  restored = true;
  await f.adapter.select("native-a");
  assert.equal(f.recent.agent.modelId, "agent:native-a");
  assert.equal(f.recent.solo_agent.modelId, "solo_agent:native-a");
  assert.deepEqual(f.sessions, { existing: { modelId: "original" } });
});

test("rejects missing, ambiguous, restricted and unselectable native models before writing", async () => {
  for (const mutate of [
    (f) => {
      f.rows.agent = [];
      f.rows.solo_agent = [];
    },
    (f) => {
      f.rows.agent.push({ ...f.rows.agent[0] });
    },
    (f) => {
      f.port.isModelSelectionRestricted = () => true;
    },
    (f) => {
      f.rows.agent[0].selectable = false;
    },
  ]) {
    const f = fixture();
    mutate(f);
    await assert.rejects(
      f.adapter.select("native-a"),
      /starting_model_unavailable/,
    );
    assert.equal(f.calls.length, 0);
  }
});

test("does not acknowledge failed persistence or a native selection mismatch", async () => {
  for (const mutate of [
    (f) => {
      f.port.persistUserModelSelection = async () => false;
    },
    (f) => {
      f.services.ModelPersistenceService.saveRecentUserSelection = async () => {
        throw Error("private-detail");
      };
    },
    (f) => {
      f.services.ModelDomainService.getUserSelectedModel = () => ({
        modelId: "other",
        mode: 0,
      });
    },
  ]) {
    const f = fixture();
    mutate(f);
    await assert.rejects(
      f.adapter.select("native-a"),
      /starting_model_unavailable/,
    );
  }
});

test("awaits legacy IDE persistence and verifies its new-session selection", async () => {
  for (const moduleId of [undefined, 7494, 99002]) {
    for (const failure of [null, "save", "model", "mode"]) {
      const f = fixture(moduleId);
      let saved = false;
      f.port.legacyModelSelectionAdapter = {
        sessionRelationStore: {
          persistGlobalModelMap: async () => {
            if (failure === "save") throw Error("fixture");
            saved = true;
          },
          persistGlobalModeMap: async () => {},
        },
        getSelectedModel: (session) => {
          assert.equal(session, undefined);
          return {
            custom_model_id: failure === "model" ? "other" : "native-a",
          };
        },
        getSelectedMode: () => (failure === "mode" ? 1 : 0),
      };
      if (failure)
        await assert.rejects(
          f.adapter.select("native-a"),
          /starting_model_unavailable/,
        );
      else {
        await f.adapter.select("native-a");
        assert.equal(saved, true);
      }
    }
  }
});
