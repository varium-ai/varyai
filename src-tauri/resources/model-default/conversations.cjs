"use strict";

// Serialized into the verified renderer. Only native model configuration is written.
async function applyConversationDefault(scope, plan) {
  if (!["workbuddy", "workbuddy-ai"].includes(plan.app))
    return { applied: true };
  const key = "__varyaiWorkBuddyConversationDefault";
  const previous = scope[key];
  if (plan.restore) {
    if (previous) previous.cancelled = true;
    delete scope[key];
    return { applied: true };
  }
  const safe = (value) =>
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !/[\u0000-\u001f\u007f]/.test(value);
  if (
    !safe(plan.model) ||
    !safe(plan.revision) ||
    typeof scope.__wbInvoke !== "function"
  )
    throw Error("model_default_unavailable");
  const model = plan.model.startsWith("custom-local:")
    ? plan.model
    : `custom-local:${plan.model}`;
  if (previous?.revision === plan.revision && previous.model === model)
    return previous.promise;
  if (previous) previous.cancelled = true;
  const state = { revision: plan.revision, model, cancelled: false };
  scope[key] = state;
  state.promise = (async () => {
    const invoke = async (method, ...args) => {
      if (state.cancelled) throw Error("model_default_unavailable");
      const result = await scope.__wbInvoke(
        `wb:conversations:${method}`,
        undefined,
        ...args,
      );
      if (
        state.cancelled ||
        result?.__wbError === true ||
        (result && typeof result === "object" && "errorCode" in result)
      )
        throw Error("model_default_unavailable");
      return result;
    };
    const ids = [],
      seen = new Set();
    let total;
    // Snapshot metadata before model writes can reorder the native conversation list.
    for (let page = 1; ; page++) {
      const result = await invoke("list", {
        transport: "local",
        page,
        size: 100,
      });
      if (
        !Array.isArray(result?.items) ||
        result.items.length > 100 ||
        !Number.isSafeInteger(result.total) ||
        result.total < 0 ||
        result.total > 10000 ||
        result.partial ||
        result.providerErrors?.length ||
        (total !== undefined && result.total !== total)
      )
        throw Error("model_default_unavailable");
      total = result.total;
      for (const item of result.items) {
        if (
          !safe(item?.id) ||
          seen.has(item.id) ||
          !["local", "cloud"].includes(item.transport)
        )
          throw Error("model_default_unavailable");
        seen.add(item.id);
        if (item.transport === "local") ids.push(item.id);
      }
      if (page * 100 >= total) break;
      if (result.items.length === 0) throw Error("model_default_unavailable");
    }
    if (seen.size !== total) throw Error("model_default_unavailable");
    for (const id of ids) {
      await invoke("configSetModel", id, model);
      const readback = await invoke("get", id);
      if (
        readback?.info?.id !== id ||
        readback.info.transport !== "local" ||
        readback.configManager?.model !== model
      )
        throw Error("model_default_unavailable");
    }
    return { applied: true };
  })().catch(() => {
    if (scope[key] === state) delete scope[key];
    throw Error("model_default_unavailable");
  });
  return state.promise;
}

module.exports = { applyConversationDefault };
