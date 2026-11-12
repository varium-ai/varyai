"use strict";
// Kept self-contained because the vendor component API executes this function in the workbench.
function createBridge(
  service,
  audit,
  refreshModels = async () => {},
  storage = globalThis.localStorage,
  startingModel,
  rewards,
) {
  let undo;
  const sessions = new Map(),
    reported = new Set(),
    controllers = new Set();
  // Native edit requests keep provider and request-model identity immutable.
  const editFields = (fields) => ({
    display_name: fields.display_name,
    ak: fields.ak,
    base_url: fields.base_url,
    auth_type: fields.auth_type,
    custom_model_type: fields.custom_model_type,
  });
  const modelFailure = (operation, code) =>
    Error(
      `model_${operation}_failed${Number.isSafeInteger(code) ? `:${code}` : ""}`,
    );
  const ownedName = async (id, model) => {
    const prefix = `VaryAI/${id}/`;
    if (prefix.length + model.length <= 64) return prefix + model;
    // TRAE limits display names to 64 characters; keep the request model ID intact.
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(model),
    );
    const hash = Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, "0"),
    ).join("");
    return prefix + hash.slice(0, 64 - prefix.length);
  };
  const loadOwned = (id) => {
    const rows = JSON.parse(
      storage.getItem(`varyai.trae.models.${id}`) || "[]",
    );
    if (
      !Array.isArray(rows) ||
      rows.some(
        (row) =>
          !row || typeof row.id !== "string" || typeof row.model !== "string",
      )
    )
      throw Error("model_ownership_unavailable");
    return rows;
  };
  const saveOwned = (id, rows) =>
    storage.setItem(
      `varyai.trae.models.${id}`,
      JSON.stringify(rows.map(({ id, model }) => ({ id, model }))),
    );
  const modelName = (model) => {
    if (typeof model.model_name === "string") return model.model_name;
    const prefix = `${model.provider}//`;
    return model.name?.startsWith(prefix)
      ? model.name.slice(prefix.length)
      : model.name;
  };
  const providerType = (protocol) =>
    ({
      openai: "custom_openai_compatible",
      "openai-responses": "custom_responses_compatible",
      anthropic: "custom_anthropic_compatible",
    })[protocol];
  const digest = async (value) => {
    const bytes = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(JSON.stringify(value)),
    );
    return Array.from(new Uint8Array(bytes), (byte) =>
      byte.toString(16).padStart(2, "0"),
    ).join("");
  };
  const expectedProof = (config, model) =>
    digest([config.protocol, endpoint(config), model, config.apiKey]);
  const nativeProof = (row) =>
    digest([
      row.custom_model_id,
      modelName(row),
      row.provider,
      row.base_url,
      row.ak,
    ]);
  const verificationKey = (id) => `varyai.trae.verification.${id}`;
  const loadVerification = (id) => {
    try {
      const rows = JSON.parse(storage.getItem(verificationKey(id)) || "[]");
      return Array.isArray(rows)
        ? rows.filter(
            (row) =>
              row &&
              typeof row.id === "string" &&
              typeof row.expected === "string" &&
              typeof row.native === "string",
          )
        : [];
    } catch {
      return [];
    }
  };
  const isOwned = (id, rows, model) =>
    rows.some((row) => row.id === model.id) ||
    model.displayName?.startsWith(`VaryAI/${id}/`);
  const legacyVerificationKey = (id) => `varyai.trae.model-verification.${id}`;
  const modelNameMatches = (wire, model) =>
    typeof wire === "string" && (wire === model || wire.endsWith(`//${model}`));
  async function modelVerification(config, wanted, row) {
    const wire = row?.model_name ?? row?.name;
    if (
      !modelNameMatches(wire, wanted.model) ||
      row.base_url !== endpoint(config) ||
      typeof row.ak !== "string" ||
      !row.ak ||
      /^[*•]+$/.test(row.ak)
    )
      return null;
    // Bind the submitted credential to the native protected representation.
    // Only a one-way digest is persisted; neither representation leaves this call.
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(
        JSON.stringify([
          config.protocol,
          wanted.model,
          config.apiKey,
          row.base_url,
          row.provider,
          wire,
          row.ak,
        ]),
      ),
    );
    return Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, "0"),
    ).join("");
  }
  const shortText = (value, limit) =>
    value.slice(0, limit).replace(/[\uD800-\uDBFF]$/u, "");
  const displayName = (model, provider, used) => {
    const label =
      model.displayName?.trim() || model.model.split("/").pop() || model.model;
    const suffix = ` · ${shortText(provider.trim() || "VaryAI", 32)}`;
    const name = label.endsWith(suffix)
      ? label.slice(0, -suffix.length)
      : label;
    let index = 1,
      value;
    do {
      const number = index === 1 ? "" : ` (${index})`;
      value =
        shortText(name, 64 - suffix.length - number.length) + number + suffix;
      index++;
    } while (used.has(value));
    used.add(value);
    return value;
  };
  async function list() {
    const result = await service.getModelList();
    if (!Array.isArray(result.model_list))
      throw Error("model_list_unavailable");
    return result.model_list.map((m) => ({
      id: m.custom_model_id,
      name: modelName(m),
      displayName: m.display_name,
      baseUrl: m.base_url,
      provider: m.provider,
    }));
  }
  async function remove(id) {
    let owned = loadOwned(id);
    await refreshModels();
    const models = await list();
    for (const m of models.filter((m) => m.id && isOwned(id, owned, m))) {
      const result = await service.updateCustomModel({
        action: "delete",
        id: m.id,
      });
      if (result.code !== 0) throw modelFailure("remove", result.code);
      owned = owned.filter((row) => row.id !== m.id);
      saveOwned(id, owned);
    }
    saveOwned(id, []);
    storage.setItem(verificationKey(id), "[]");
    storage.setItem(legacyVerificationKey(id), "{}");
    await refreshModels();
  }
  function endpoint(config) {
    const suffix = {
      openai: "/chat/completions",
      "openai-responses": "/responses",
      anthropic: "/v1/messages",
    }[config.protocol];
    if (!suffix) throw Error("invalid_protocol");
    let baseUrl = config.baseUrl.trim().replace(/\/+$/, "");
    if (!baseUrl.endsWith(suffix))
      baseUrl +=
        config.protocol === "anthropic" && baseUrl.endsWith("/v1")
          ? "/messages"
          : suffix;
    return baseUrl;
  }
  async function matches(id, config) {
    if (!/^[\w-]+$/.test(id) || !Array.isArray(config.models)) return false;
    await refreshModels();
    const result = await service.getModelList();
    if (!Array.isArray(result.model_list)) return false;
    const owned = loadOwned(id),
      verification = loadVerification(id),
      baseUrl = endpoint(config);
    let legacyVerification = {};
    try {
      legacyVerification = JSON.parse(
        storage.getItem(legacyVerificationKey(id)) || "{}",
      );
    } catch {}
    return (
      owned.length === config.models.length &&
      (
        await Promise.all(
          config.models.map(async (wanted) => {
            const owner = owned.find((row) => row.model === wanted.model);
            const row =
              owner &&
              result.model_list.find((m) => m.custom_model_id === owner.id);
            if (
              !row ||
              !modelNameMatches(modelName(row), wanted.model) ||
              row.base_url !== baseUrl ||
              row.provider !== providerType(config.protocol) ||
              typeof row.ak !== "string" ||
              !row.ak ||
              /^[*•]+$/.test(row.ak)
            )
              return false;
            if (row.ak === config.apiKey) return true;
            // TRAE returns sealed keys. Verify the result of our successful write,
            // without persisting either the local credential or the sealed value.
            const proof = verification.find(
              (entry) => entry.id === row.custom_model_id,
            );
            if (
              proof &&
              proof.expected === (await expectedProof(config, wanted.model)) &&
              proof.native === (await nativeProof(row))
            )
              return true;
            return (
              typeof legacyVerification?.[row.custom_model_id] === "string" &&
              legacyVerification[row.custom_model_id] ===
                (await modelVerification(config, wanted, row))
            );
          }),
        )
      ).every(Boolean)
    );
  }
  async function apply(id, name, config) {
    if (!/^[\w-]+$/.test(id) || !Array.isArray(config.models))
      throw Error("invalid_provider");
    const baseUrl = endpoint(config);
    let owned = loadOwned(id);
    // Native writes do not invalidate TRAE's model-list cache automatically.
    await refreshModels();
    const existing = await list();
    owned = owned.filter((row) =>
      existing.some((model) => model.id === row.id),
    );
    const used = new Set(
      existing
        .filter((model) => !isOwned(id, owned, model))
        .map((model) => model.displayName),
    );
    const labels = new Set();
    const desiredLabels = config.models.map((model) =>
      displayName(model, name, labels),
    );
    const added = [];
    const addedIds = new Set(),
      wanted = new Set();
    try {
      for (const [index, model] of config.models.entries()) {
        const legacyName = await ownedName(id, model.model);
        const previous = owned.find((row) => row.model === model.model);
        let match =
          (previous && existing.find((m) => m.id === previous.id)) ||
          existing.find((m) => m.id && m.displayName === legacyName);
        if (!match && id === "shared_catalog_connection") {
          const url = new URL(baseUrl);
          const candidates = existing.filter(
            (m) =>
              m.id &&
              !owned.some((row) => row.id === m.id) &&
              modelNameMatches(m.name, model.model) &&
              m.baseUrl === baseUrl &&
              m.provider === providerType(config.protocol) &&
              m.displayName === desiredLabels[index],
          );
          if (
            url.protocol === "http:" &&
            url.hostname === "127.0.0.1" &&
            [
              "/connections/trae/v1/chat/completions",
              "/connections/trae-intl/v1/chat/completions",
            ].includes(url.pathname) &&
            !url.username &&
            !url.password &&
            !url.search &&
            !url.hash &&
            candidates.length === 1
          ) {
            match = candidates[0];
            used.delete(match.displayName);
          }
        }
        const display_name = displayName(model, name, used);
        const fields = {
          provider: providerType(config.protocol),
          model_name: model.model,
          display_name,
          ak: config.apiKey,
          // Native storage expects the final URL; the TRAE form normally appends this path.
          base_url: baseUrl,
          is_custom: true,
          auth_type: 0,
          custom_model_type: "chat_model",
        };
        if (!fields.provider) throw Error("invalid_protocol");
        if (!match) {
          // Shared rows can recover ownership by their unique local endpoint and wire identity.
          // Publish their final label immediately so refreshes never expose a temporary hash.
          const initialName =
            id === "shared_catalog_connection" ? display_name : legacyName;
          const result = await service.addCustomModel({
            ...fields,
            display_name: initialName,
          });
          if (result.code !== 0) throw modelFailure("add", result.code);
          if (initialName === legacyName) added.push(legacyName);
          await refreshModels();
          const candidates = (await list()).filter(
            (m) =>
              m.id &&
              !existing.some((old) => old.id === m.id) &&
              m.displayName === initialName &&
              (initialName === legacyName ||
                (m.baseUrl === baseUrl &&
                  m.provider === fields.provider &&
                  modelNameMatches(m.name, model.model))),
          );
          match = candidates.length === 1 ? candidates[0] : undefined;
          if (!match) throw Error("model_apply_failed");
          addedIds.add(match.id);
        }
        owned = owned.filter(
          (row) => row.model !== model.model && row.id !== match.id,
        );
        owned.push({ id: match.id, model: model.model });
        // Persist before replacing a legacy label, including during partial migrations.
        saveOwned(id, owned);
        if (
          match.provider !== fields.provider ||
          !modelNameMatches(match.name, model.model)
        )
          throw Error("model_identity_changed");
        if (!(id === "shared_catalog_connection" && addedIds.has(match.id))) {
          const result = await service.updateCustomModel({
            action: "update",
            id: match.id,
            ...editFields(fields),
          });
          if (result.code !== 0) throw modelFailure("update", result.code);
        }
        wanted.add(match.id);
      }
      const all = await list();
      for (const old of all.filter(
        (m) => m.id && isOwned(id, owned, m) && !wanted.has(m.id),
      )) {
        const result = await service.updateCustomModel({
          action: "delete",
          id: old.id,
        });
        if (result.code !== 0) throw modelFailure("remove", result.code);
      }
      saveOwned(
        id,
        owned.filter((row) => wanted.has(row.id)),
      );
      await refreshModels();
      const native = (await service.getModelList()).model_list;
      if (!Array.isArray(native)) throw Error("model_list_unavailable");
      const verification = await Promise.all(
        config.models.map(async (model) => {
          const owner = owned.find((row) => row.model === model.model);
          const row = native.find((row) => row.custom_model_id === owner?.id);
          if (
            !row ||
            !modelNameMatches(modelName(row), model.model) ||
            row.base_url !== baseUrl ||
            row.provider !== providerType(config.protocol) ||
            typeof row.ak !== "string" ||
            !row.ak ||
            /^[*•]+$/.test(row.ak)
          )
            throw Error("model_apply_failed");
          return {
            id: row.custom_model_id,
            expected: await expectedProof(config, model.model),
            native: await nativeProof(row),
          };
        }),
      );
      storage.setItem(verificationKey(id), JSON.stringify(verification));
      return all.filter((m) => wanted.has(m.id));
    } catch (error) {
      // Roll back newly created entries only. Never remove pre-existing native models.
      const all = await list();
      for (const m of all.filter(
        (m) => addedIds.has(m.id) || added.includes(m.displayName),
      ))
        await service.updateCustomModel({ action: "delete", id: m.id });
      try {
        saveOwned(
          id,
          owned.filter((row) => !addedIds.has(row.id)),
        );
      } catch {}
      throw error;
    }
  }
  async function replace(id, name, config) {
    if (
      config !== null &&
      (!/^[\w-]+$/.test(id) ||
        !["openai", "openai-responses", "anthropic"].includes(
          config.protocol,
        ) ||
        typeof config.baseUrl !== "string" ||
        !config.baseUrl.trim() ||
        typeof config.apiKey !== "string" ||
        !config.apiKey.trim() ||
        !Array.isArray(config.models) ||
        !config.models.length ||
        config.models.some(
          (model) => typeof model.model !== "string" || !model.model.trim(),
        ))
    )
      throw Error("invalid_provider");
    const ownership = new Map();
    for (let index = 0; index < storage.length; index++) {
      const key = storage.key(index);
      if (key?.startsWith("varyai.trae.models.")) {
        const owner = key.slice("varyai.trae.models.".length);
        ownership.set(owner, loadOwned(owner));
      }
    }
    await refreshModels();
    const result = await service.getModelList();
    if (!Array.isArray(result.model_list))
      throw Error("model_list_unavailable");
    const snapshots = [];
    for (const model of result.model_list) {
      let owner = [...ownership].find(([, rows]) =>
        rows.some((row) => row.id === model.custom_model_id),
      )?.[0];
      owner ??= /^VaryAI\/([\w-]+)\//.exec(model.display_name || "")?.[1];
      if (!owner || !model.custom_model_id) continue;
      const wire = modelName(model);
      // Recovery credentials stay in this call's memory, never in ownership storage or IPC results.
      if (
        typeof model.ak !== "string" ||
        !model.ak ||
        /^[*•]+$/.test(model.ak) ||
        typeof wire !== "string" ||
        !wire ||
        typeof model.base_url !== "string"
      ) {
        throw Error("model_recovery_unavailable");
      }
      snapshots.push({
        owner,
        id: model.custom_model_id,
        fields: {
          provider: model.provider,
          model_name: wire,
          display_name: model.display_name,
          ak: model.ak,
          base_url: model.base_url,
          is_custom: true,
          auth_type: model.auth_type ?? 0,
          custom_model_type: model.custom_model_type || "chat_model",
        },
      });
      if (!ownership.has(owner)) ownership.set(owner, []);
    }
    try {
      for (const model of snapshots) {
        const deleted = await service.updateCustomModel({
          action: "delete",
          id: model.id,
        });
        if (deleted.code !== 0) throw modelFailure("remove", deleted.code);
      }
      for (const owner of ownership.keys()) saveOwned(owner, []);
      await refreshModels();
      return config === null ? [] : await apply(id, name, config);
    } catch (error) {
      let restored = true;
      await refreshModels();
      const remaining = await list();
      for (const model of snapshots) {
        if (remaining.some((row) => row.id === model.id)) continue;
        try {
          const shared = model.owner === "shared_catalog_connection";
          const marker = shared
            ? model.fields.display_name
            : await ownedName(model.owner, model.fields.model_name);
          const added = await service.addCustomModel({
            ...model.fields,
            display_name: marker,
          });
          if (added.code !== 0) throw modelFailure("restore", added.code);
          await refreshModels();
          const candidates = (await list()).filter(
            (row) =>
              row.id &&
              !remaining.some((old) => old.id === row.id) &&
              row.displayName === marker &&
              (!shared ||
                (row.baseUrl === model.fields.base_url &&
                  row.provider === model.fields.provider &&
                  modelNameMatches(row.name, model.fields.model_name))),
          );
          const found = candidates.length === 1 ? candidates[0] : undefined;
          if (!found?.id) throw Error("model_restore_failed");
          const rows = ownership
            .get(model.owner)
            .filter((row) => row.id !== model.id);
          rows.push({ id: found.id, model: model.fields.model_name });
          ownership.set(model.owner, rows);
          saveOwned(model.owner, rows);
          const updated = await service.updateCustomModel({
            action: "update",
            id: found.id,
            ...editFields(model.fields),
          });
          if (updated.code !== 0) throw modelFailure("restore", updated.code);
        } catch {
          restored = false;
        }
      }
      await refreshModels();
      if (!restored) throw Error("model_restore_failed");
      throw error;
    }
  }
  // A timed-out IPC caller must not race a still-running native model transaction.
  let changes = Promise.resolve();
  const serial =
    (operation) =>
    (...args) => {
      const result = changes.then(() => operation(...args));
      changes = result.catch(() => {});
      return result;
    };
  const bridge = {
    list,
    matches: serial(matches),
    apply: serial(apply),
    replace: serial(replace),
    remove: serial(remove),
    select: serial(async (id, model) => {
      const owned = loadOwned(id).filter((row) => row.model === model);
      if (owned.length !== 1 || !startingModel)
        throw Error("starting_model_unavailable");
      await refreshModels();
      const rows = (await list()).filter(
        (row) => row.id === owned[0].id && row.name === model,
      );
      if (rows.length !== 1) throw Error("starting_model_unavailable");
      return startingModel.select(owned[0].id);
    }),
    idle() {
      return changes;
    },
    get enabled() {
      return Boolean(undo);
    },
    rewards(input) {
      return rewards ? rewards(input) : { outcome: "unsupported" };
    },
    disable() {
      undo?.();
      undo = undefined;
    },
    async enable() {
      if (undo) return;
      const client = await service._clientManagerService.getClient();
      const proto = Object.getPrototypeOf(client);
      let convertName = "convertReqToRustPayload";
      let original = proto[convertName];
      if (
        typeof original !== "function" &&
        typeof proto.request === "function"
      ) {
        // Newer clients minify private names; the request envelope retains its protocol keys.
        const matches = [
          ...Function.prototype.toString
            .call(proto.request)
            .matchAll(/\bpayload\s*:\s*await\s+this\.([A-Za-z_$][\w$]*)\s*\(/g),
        ];
        if (matches.length === 1) {
          const name = matches[0][1];
          const candidate = Object.getOwnPropertyDescriptor(proto, name)?.value;
          if (typeof candidate === "function") {
            const source = Function.prototype.toString.call(candidate);
            if (/\bservice\s*:/.test(source) && /\bmethod\s*:/.test(source)) {
              convertName = name;
              original = candidate;
            }
          }
        }
      }
      if (typeof original !== "function")
        throw Error("unsupported_client_version");
      const wrapped = async function (request, ...args) {
        const initial =
          request.service === "lite" &&
          request.method === "create_chat_session" &&
          request.data?.initial_message;
        const chat =
          initial ||
          (request.service === "lite" && request.method === "send_message") ||
          (request.service === "chat" && request.method === "chat");
        if (!chat) return original.call(this, request, ...args);
        const data = initial || request.data || {},
          slots = [];
        if (data.chat_session_id) {
          sessions.set(data.chat_session_id, {
            model: data.model_name,
            started: Date.now(),
          });
          if (sessions.size > 256)
            sessions.delete(sessions.keys().next().value);
        }
        if (typeof data.query === "string") slots.push(["query", data.query]);
        if (typeof data.content === "string")
          slots.push(["content", data.content]);
        else if (Array.isArray(data.content))
          data.content.forEach((part, index) => {
            if (typeof part?.text_content === "string")
              slots.push([index, part.text_content]);
          });
        if (!slots.length) return original.call(this, request, ...args);
        let result;
        try {
          result = await audit({
            sessionId: data.chat_session_id || request.context?.chat_session_id,
            model: data.model_name,
            body: {
              messages: [
                {
                  role: "user",
                  content: slots.map(([, text]) => ({ type: "text", text })),
                },
              ],
            },
          });
        } catch {
          bridge.disable();
          return original.call(this, request, ...args);
        }
        if (result.blocked)
          throw Error("[VaryAI] Sensitive data cannot be safely sent.");
        const texts = result.body?.messages?.[0]?.content;
        if (!Array.isArray(texts) || texts.length !== slots.length)
          return original.call(this, request, ...args);
        const target = { ...data };
        const next = {
          ...request,
          data: initial ? { ...request.data, initial_message: target } : target,
        };
        if (Array.isArray(data.content))
          target.content = data.content.map((x) => ({ ...x }));
        for (let i = 0; i < slots.length; i++) {
          if (typeof texts[i]?.text !== "string")
            throw Error("[VaryAI] Invalid audit response.");
          const key = slots[i][0];
          if (typeof key === "number")
            target.content[key].text_content = texts[i].text;
          else target[key] = texts[i].text;
        }
        return original.call(this, next, ...args);
      };
      const originalStream = proto.requestForStream;
      const wrapStream = function (request, ...args) {
        const stream = originalStream.call(this, request, ...args);
        if (request.service !== "lite" || request.method !== "subscribe_events")
          return stream;
        const controller = stream.getController(),
          write = controller.write;
        const sessionId = request.data?.chat_session_id,
          state = sessions.get(sessionId) || {};
        let usage,
          model = state.model;
        const restore = () => {
          if (controller.write === observe) controller.write = write;
          controllers.delete(restore);
        };
        const observe = function (value) {
          try {
            const event = value?.data || value;
            let payload = event?.payload;
            if (typeof payload === "string") payload = JSON.parse(payload);
            if (
              event?.event === "model_config" &&
              typeof payload?.model_name === "string"
            )
              model = payload.model_name;
            if (event?.event === "token_usage" && payload) {
              const number = (...keys) => {
                for (const key of keys) {
                  const raw = payload[key];
                  const value =
                    typeof raw === "string" && /^\d{1,15}$/.test(raw)
                      ? Number(raw)
                      : raw;
                  if (Number.isSafeInteger(value) && value > 0) return value;
                }
                return 0;
              };
              usage = {
                input_tokens: number(
                  "prompt_tokens_total",
                  "prompt_tokens",
                  "input",
                ),
                output_tokens: number(
                  "completion_tokens_total",
                  "completion_tokens",
                  "output",
                ),
                cache_read_tokens: number("cache_read_input_tokens"),
                cache_creation_tokens: number("cache_creation_input_tokens"),
              };
            }
            if (event?.event === "done") {
              const requestId =
                payload?.reply_to_message_id ||
                request.data?.reply_to_message_id;
              if (
                bridge.enabled &&
                usage &&
                requestId &&
                !reported.has(requestId)
              ) {
                reported.add(requestId);
                if (reported.size > 512)
                  reported.delete(reported.values().next().value);
                Promise.resolve(
                  audit({
                    usage,
                    sessionId,
                    model,
                    requestId,
                    latencyMs: state.started ? Date.now() - state.started : 0,
                  }),
                ).catch(() => {});
              }
              restore();
            }
          } catch {}
          return write.call(this, value);
        };
        controller.write = observe;
        controllers.add(restore);
        return stream;
      };
      proto[convertName] = wrapped;
      if (typeof originalStream === "function")
        proto.requestForStream = wrapStream;
      undo = () => {
        if (proto[convertName] === wrapped) proto[convertName] = original;
        if (proto.requestForStream === wrapStream)
          proto.requestForStream = originalStream;
        for (const restore of controllers) restore();
        sessions.clear();
        reported.clear();
      };
    },
  };
  return bridge;
}
function workbenchServices(req) {
  const ids = Object.keys(req.m || {}).filter((id) =>
    /\bmcf\s*:/.test(Function.prototype.toString.call(req.m[id])),
  );
  if (ids.length !== 1) throw Error("unsupported_ide_version");
  const di = req(ids[0]).mcf?.getInstance();
  if (!di?.isInitialized() || typeof di.resolve !== "function")
    throw Error("unsupported_ide_version");
  const service = di.resolve(Symbol.for("aiAgent.IAiNativeModelService"));
  const store = di.resolve(Symbol.for("IModelService"));
  const management = di.resolve("iICubeModelManagementService");
  if (
    typeof service?.getModelList !== "function" ||
    typeof store?.refreshModelStore !== "function" ||
    typeof management?.notifyModelChanges !== "function"
  )
    throw Error("unsupported_ide_version");
  return {
    service,
    async refresh() {
      await store.refreshModelStore({ source: "varyai" });
      management.notifyModelChanges({ source: "varyai" });
    },
  };
}
function rendererSource() {
  return `(()=>{
const tag='varyai-trae-bridge-v1';if(customElements.get(tag))return;
const createBridge=${createBridge.toString()};
customElements.define(tag,class extends HTMLElement{
 constructor(){super();this.pending=new Map();this.addEventListener('icube',e=>this.receive(e.detail))}
 connectedCallback(){this.style.display='none'}
 send(value){this.dispatchEvent(new CustomEvent('icube-component',{detail:value}))}
 async init(){if(!this.ready)this.ready=(async()=>{
  const mod=await import(location.href.split('/out/')[0]+'/node_modules/@byted-icube/ai-modules-chat/dist/index.mjs');
  const req=mod.__webpack_require__, workbenchServices=${workbenchServices.toString()};
  const {service,refresh}=workbenchServices(req);
  return createBridge(service,value=>new Promise((resolve,reject)=>{
   const id=crypto.randomUUID(),timer=setTimeout(()=>{this.pending.delete(id);reject(Error('audit_unavailable'))},5000);
   this.pending.set(id,{resolve,reject,timer});this.send({audit:id,value});
  }),refresh);})();return this.ready}
 async receive(message){
  if(message.auditResult){const pending=this.pending.get(message.auditResult);if(pending){clearTimeout(pending.timer);this.pending.delete(message.auditResult);message.error?pending.reject(Error('audit_unavailable')):pending.resolve(message.value)}return}
  try{const bridge=await this.init();let result;
   switch(message.op){case'health':result={ready:true,enabled:bridge.enabled};break;
    case'enable':await bridge.enable();result=true;break;case'disable':bridge.disable();result=true;break;
    case'list':result=await bridge.list();break;case'matches':result=await bridge.matches(message.id,message.config);break;case'apply':result=await bridge.apply(message.id,message.name,message.config);break;
    case'replace':result=await bridge.replace(message.id,message.name,message.config);break;case'remove':await bridge.remove(message.id);result=true;break;default:throw Error('unsupported_operation')}
   this.send({reply:message.request,result});
  }catch{this.send({reply:message.request,error:'TRAE native bridge unavailable'})}
 }
});})();`;
}
module.exports = { createBridge, rendererSource, workbenchServices };
