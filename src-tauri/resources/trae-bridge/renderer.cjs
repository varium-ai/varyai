"use strict";
// Kept self-contained because the vendor component API executes this function in the workbench.
function createBridge(
  service,
  audit,
  refreshModels = async () => {},
  storage = globalThis.localStorage,
) {
  let undo;
  const sessions = new Map(),
    reported = new Set(),
    controllers = new Set();
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
  const isOwned = (id, rows, model) =>
    rows.some((row) => row.id === model.id) ||
    model.displayName?.startsWith(`VaryAI/${id}/`);
  const shortText = (value, limit) =>
    value.slice(0, limit).replace(/[\uD800-\uDBFF]$/u, "");
  const displayName = (model, provider, used) => {
    const label =
      model.displayName?.trim() || model.model.split("/").pop() || model.model;
    const suffix = ` · ${shortText(provider.trim() || "VaryAI", 32)}`;
    let index = 1,
      value;
    do {
      const number = index === 1 ? "" : ` (${index})`;
      value =
        shortText(label, 64 - suffix.length - number.length) + number + suffix;
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
      name: m.name,
      displayName: m.display_name,
      baseUrl: m.base_url,
      provider: m.provider,
    }));
  }
  async function remove(id) {
    let owned = loadOwned(id);
    const models = await list();
    for (const m of models.filter((m) => m.id && isOwned(id, owned, m))) {
      const result = await service.updateCustomModel({
        action: "delete",
        id: m.id,
      });
      if (result.code !== 0) throw Error("model_remove_failed");
      owned = owned.filter((row) => row.id !== m.id);
      saveOwned(id, owned);
    }
    saveOwned(id, []);
    await refreshModels();
  }
  async function apply(id, name, config) {
    if (!/^[\w-]+$/.test(id) || !Array.isArray(config.models))
      throw Error("invalid_provider");
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
    const added = [];
    const addedIds = new Set(),
      wanted = new Set();
    try {
      for (const model of config.models) {
        const legacyName = await ownedName(id, model.model);
        const previous = owned.find((row) => row.model === model.model);
        let match =
          (previous && existing.find((m) => m.id === previous.id)) ||
          existing.find((m) => m.id && m.displayName === legacyName);
        const display_name = displayName(model, name, used);
        const fields = {
          provider: {
            openai: "custom_openai_compatible",
            "openai-responses": "custom_responses_compatible",
            anthropic: "custom_anthropic_compatible",
          }[config.protocol],
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
          // Keep the recoverable legacy marker until native ownership is persisted.
          const result = await service.addCustomModel({
            ...fields,
            display_name: legacyName,
          });
          if (result.code !== 0) throw Error("model_apply_failed");
          added.push(legacyName);
          await refreshModels();
          match = (await list()).find(
            (m) => m.displayName === legacyName && m.id,
          );
          if (!match) throw Error("model_apply_failed");
          addedIds.add(match.id);
        }
        owned = owned.filter(
          (row) => row.model !== model.model && row.id !== match.id,
        );
        owned.push({ id: match.id, model: model.model });
        // Persist before replacing a legacy label, including during partial migrations.
        saveOwned(id, owned);
        const result = await service.updateCustomModel({
          action: "update",
          id: match.id,
          ...fields,
        });
        if (result.code !== 0) throw Error("model_apply_failed");
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
        if (result.code !== 0) throw Error("model_remove_failed");
      }
      saveOwned(
        id,
        owned.filter((row) => wanted.has(row.id)),
      );
      await refreshModels();
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
  const bridge = {
    list,
    apply,
    remove,
    get enabled() {
      return Boolean(undo);
    },
    disable() {
      undo?.();
      undo = undefined;
    },
    async enable() {
      if (undo) return;
      const client = await service._clientManagerService.getClient();
      const proto = Object.getPrototypeOf(client),
        original = proto.convertReqToRustPayload;
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
      const wrapStream = function (request) {
        const stream = originalStream.call(this, request);
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
      proto.convertReqToRustPayload = wrapped;
      if (typeof originalStream === "function")
        proto.requestForStream = wrapStream;
      undo = () => {
        if (proto.convertReqToRustPayload === wrapped)
          proto.convertReqToRustPayload = original;
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
  const req=mod.__webpack_require__, service=req(20536).mc.getInstance().resolve(req(94656).b3);
  return createBridge(service,value=>new Promise((resolve,reject)=>{
   const id=crypto.randomUUID(),timer=setTimeout(()=>{this.pending.delete(id);reject(Error('audit_unavailable'))},5000);
   this.pending.set(id,{resolve,reject,timer});this.send({audit:id,value});
  }),()=>req(20536).mc.getInstance().resolve(req(1053).S).refreshModelStore({source:'varyai'}));})();return this.ready}
 async receive(message){
  if(message.auditResult){const pending=this.pending.get(message.auditResult);if(pending){clearTimeout(pending.timer);this.pending.delete(message.auditResult);message.error?pending.reject(Error('audit_unavailable')):pending.resolve(message.value)}return}
  try{const bridge=await this.init();let result;
   switch(message.op){case'health':result={ready:true,enabled:bridge.enabled};break;
    case'enable':await bridge.enable();result=true;break;case'disable':bridge.disable();result=true;break;
    case'list':result=await bridge.list();break;case'apply':result=await bridge.apply(message.id,message.name,message.config);break;
    case'remove':await bridge.remove(message.id);result=true;break;default:throw Error('unsupported_operation')}
   this.send({reply:message.request,result});
  }catch{this.send({reply:message.request,error:'TRAE native bridge unavailable'})}
 }
});})();`;
}
module.exports = { createBridge, rendererSource };
