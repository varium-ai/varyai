"use strict";

// This function is serialized into the verified desktop renderer. It has no imports.
function applyDefault(scope, plan) {
  const journalKey = "__varyaiModelDefaultV1";
  const safe = (value) =>
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !/[\u0000-\u001f\u007f?#@\\]/.test(value) &&
    !value.includes("://");
  if (
    !["workbuddy", "workbuddy-ai", "qoder"].includes(plan.app) ||
    !safe(plan.revision) ||
    !safe(plan.model) ||
    (plan.app === "qoder" &&
      !plan.restore &&
      (!safe(plan.nativeKey) || !plan.nativeKey.startsWith("byok:")))
  )
    throw Error("model_default_unavailable");
  const base =
    plan.app === "qoder"
      ? "qoder:chat-composer:selected-model"
      : "cb-newtask:model";
  const eligible = (key) =>
    key === base ||
    (plan.app === "qoder"
      ? /^qoder:chat-composer:selected-model:account:[^:]{1,512}$/
      : /^cb-newtask:model:[^:]{1,512}$/
    ).test(key);
  const validValue = (text) => {
    if (text === null) return true;
    if (typeof text !== "string" || text.length > 4096) return false;
    try {
      const value = JSON.parse(text);
      if (plan.app === "qoder") return safe(value);
      return (
        !!value &&
        !Array.isArray(value) &&
        safe(value.id) &&
        Object.keys(value).every((key) =>
          ["id", "isThinking", "reasoningEffort", "contextWindow"].includes(
            key,
          ),
        ) &&
        (value.isThinking === undefined ||
          typeof value.isThinking === "boolean") &&
        (value.reasoningEffort === undefined ||
          value.reasoningEffort === null ||
          safe(value.reasoningEffort)) &&
        (value.contextWindow === undefined ||
          Number.isSafeInteger(value.contextWindow))
      );
    } catch {
      return false;
    }
  };
  const storage = scope.localStorage;
  const previous = scope.__varyaiModelDefault;
  if (previous && previous.revision === plan.revision && !plan.restore)
    return { applied: true };
  const raw = previous?.raw ?? {
    get: storage.getItem.bind(storage),
    set: storage.setItem.bind(storage),
    remove: storage.removeItem.bind(storage),
  };
  const text = raw.get(journalKey);
  const journal =
    text === null ? { version: 1, entries: {} } : JSON.parse(text);
  if (
    text?.length > 1024 * 1024 ||
    journal?.version !== 1 ||
    !journal.entries ||
    Array.isArray(journal.entries) ||
    Object.keys(journal.entries).length > 128 ||
    Object.entries(journal.entries).some(
      ([key, value]) =>
        !eligible(key) ||
        !validValue(value.before) ||
        !validValue(value.installed),
    )
  )
    throw Error("model_default_unavailable");
  const entries = Object.keys(journal.entries);
  const keys = [
    ...new Set([
      base,
      ...Array.from({ length: storage.length }, (_, i) =>
        storage.key(i),
      ).filter((key) => typeof key === "string" && eligible(key)),
    ]),
  ];
  if (keys.length > 128 || keys.some((key) => !validValue(raw.get(key))))
    throw Error("model_default_unavailable");
  previous?.dispose();
  if (plan.restore) {
    for (const key of entries) {
      const entry = journal.entries[key];
      if (raw.get(key) !== entry.installed) continue;
      entry.before === null ? raw.remove(key) : raw.set(key, entry.before);
    }
    raw.remove(journalKey);
    delete scope.__varyaiModelDefault;
    return { applied: true };
  }
  const installed = JSON.stringify(
    plan.app === "qoder"
      ? plan.nativeKey
      : {
          id: plan.model.startsWith("custom-local:")
            ? plan.model
            : `custom-local:${plan.model}`,
          isThinking: false,
        },
  );
  const seen = new Set();
  const seed = (key) => {
    if (seen.has(key) || !eligible(key)) return;
    const current = raw.get(key);
    if (
      !validValue(current) ||
      (Object.keys(journal.entries).length >= 128 &&
        !Object.hasOwn(journal.entries, key))
    )
      throw Error("model_default_unavailable");
    const prior = journal.entries[key];
    // Preserve a native choice made since our last write as the new recovery baseline.
    const before =
      prior && current === prior.installed ? prior.before : current;
    journal.entries[key] = { before, installed };
    raw.set(journalKey, JSON.stringify(journal));
    raw.set(key, installed);
    if (raw.get(key) !== installed) throw Error("model_default_unavailable");
    seen.add(key);
  };
  for (const key of keys) seed(key);
  const prototype = scope.Storage?.prototype ?? storage;
  const descriptor = Object.getOwnPropertyDescriptor(prototype, "getItem");
  const original = prototype.getItem;
  const wrapped = function (key) {
    if (this === storage && typeof key === "string" && eligible(key)) {
      try {
        seed(key);
      } catch {
        /* Native reads must remain available. */
      }
    }
    return original.call(this, key);
  };
  Object.defineProperty(prototype, "getItem", {
    configurable: true,
    writable: true,
    value: wrapped,
  });
  scope.__varyaiModelDefault = {
    revision: plan.revision,
    raw,
    dispose() {
      if (prototype.getItem === wrapped) {
        if (descriptor) Object.defineProperty(prototype, "getItem", descriptor);
        else delete prototype.getItem;
      }
    },
  };
  return { applied: true };
}

module.exports = { applyDefault };
