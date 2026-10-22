// Public models.json and ModelRuntime only; all credentials and endpoints are fixtures.
import assert from "node:assert/strict";
import path from "node:path";
import { pathToFileURL } from "node:url";
const [root, directory] = process.argv.slice(2);
const { ModelRuntime } = await import(
  pathToFileURL(path.join(root, "dist/index.js"))
);
const runtime = await ModelRuntime.create({
  modelsPath: path.join(directory, "models.json"),
  authPath: path.join(directory, "auth.json"),
  modelsStorePath: path.join(directory, "models-store.json"),
  allowModelNetwork: false,
  refreshOnCreate: false,
});
assert.equal(runtime.getError(), undefined);
const models = ["chat", "responses", "anthropic", "gemini"].map((provider) => {
  const id = provider === "gemini" ? "gemini-2.5-pro" : `model-${provider}`;
  const model = runtime
    .getModels(provider)
    .find((candidate) => candidate.id === id);
  assert(model, `${provider}/${id} must retain its native identity`);
  return model;
});
assert.equal(models.length, 4);
for (const model of models) {
  const user = { role: "user", content: "hello", timestamp: Date.now() };
  const tools = [
    {
      name: "dynamic_tool",
      description: "Added by an extension",
      parameters: {
        type: "object",
        properties: { value: { type: "string" } },
        required: ["value"],
      },
    },
  ];
  const options = {
    reasoning: "high",
    maxTokens: 4096,
    maxRetries: 0,
    transport: "sse",
    headers: { "x-pi-native-smoke": "first" },
  };
  const response = await runtime
    .streamSimple(model, { messages: [user], tools }, options)
    .result();
  assert.notEqual(
    response.stopReason,
    "error",
    `${model.api}: ${response.errorMessage}`,
  );
  assert(
    response.content.some((c) => c.type === "text" && c.text === "ok"),
    `${model.api}: ${JSON.stringify(response)}`,
  );
  assert.equal(response.provider, model.provider);
  assert.equal(
    response.model,
    model.id,
    "session identity must keep the original model",
  );
  tools.push({ ...tools[0], name: "later_tool" });
  const next = await runtime
    .streamSimple(
      model,
      { messages: [user, response, { ...user, content: "continue" }], tools },
      { ...options, headers: { "x-pi-native-smoke": "second" } },
    )
    .result();
  assert.notEqual(
    next.stopReason,
    "error",
    `${model.api}: ${next.errorMessage}`,
  );
  console.log(
    `${model.api}: SSE, thinking, dynamic tools, and session identity verified`,
  );
}
