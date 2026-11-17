const test = require("node:test");
const assert = require("node:assert/strict");
const { Readable } = require("node:stream");
const { project, run } = require("./tool-risk-hook.cjs");

test("only native pre-execution shell calls reach the observer", () => {
  const value = {
    hook_event_name: "PreToolUse",
    tool_name: "RunCommand",
    tool_input: {
      command: "kubectl delete pod synthetic --dry-run=client",
      env: { private: "synthetic-private-marker" },
    },
    session_id: "synthetic-session",
    tool_use_id: "synthetic-call",
    unrelated: "synthetic-private-marker",
  };
  const result = project(value);
  assert.equal(result.session_id, "synthetic-session");
  assert.equal(result.tool_use_id, "synthetic-call");
  assert.equal(result.unrelated, undefined);
  assert.equal(result.tool_input.env, undefined);
  for (const change of [
    { hook_event_name: "PostToolUse" },
    { tool_name: "mcp__synthetic__RunCommand" },
    { tool_input: { command: ["kubectl"] } },
  ])
    assert.equal(project({ ...value, ...change }), null);
});

test("missing observer configuration never rejects or produces a decision", async () => {
  assert.equal(
    await run("/synthetic/missing/config.json", Readable.from(["{}"])),
    undefined,
  );
});
