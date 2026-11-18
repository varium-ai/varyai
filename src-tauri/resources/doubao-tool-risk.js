// Resolve the installed native lifecycle by its contract, without reading chats or credentials.
function varyaiDoubaoToolLifecycle() {
  const arrays = [
    ...new Set(
      [
        globalThis["chunk_@flow-web/desktop"],
        globalThis["@flow-web/desktop:stable"],
      ].filter(Array.isArray),
    ),
  ];
  if (arrays.length !== 1) return null;
  const chunks = arrays[0];
  const cache = Symbol.for("varyai.toolRisk.runtime");
  let require = chunks[cache];
  if (!require) {
    chunks.push([
      [`varyai-tool-risk-${Date.now()}`],
      {},
      (runtime) => {
        require = runtime;
      },
    ]);
    chunks.pop();
    if (require?.m) Object.defineProperty(chunks, cache, { value: require });
  }
  if (!require?.m) return null;
  const modules = Object.entries(require.m).filter(([, factory]) => {
    const source = factory.toString();
    return (
      source.includes("ToolCallManager has been destroyed") &&
      source.includes("onBeforeExecution")
    );
  });
  if (modules.length !== 1) return null;
  const [id, factory] = modules[0];
  const match = factory
    .toString()
    .match(/([A-Za-z_$][\w$]*)=\[\{(?:async )?onBeforeExecution\(/);
  const entry = require(id).default;
  return match && typeof entry === "function"
    ? { entry, slot: match[1] }
    : null;
}

function varyaiInstallDoubaoToolObserver(binding) {
  if (
    !Array.isArray(this) ||
    !this.some((hook) => typeof hook?.onBeforeExecution === "function") ||
    !this.some((hook) => typeof hook?.onExecutionComplete === "function") ||
    typeof globalThis[binding] !== "function"
  )
    return false;
  const slot = Symbol.for("varyai.doubao.toolRisk");
  globalThis[slot]?.dispose();
  const lifecycle = this;
  const hook = {
    onBeforeExecution({ ctx }) {
      try {
        const request = ctx?.request;
        const command = request?.input?.command;
        if (
          request?.name !== "Bash" ||
          typeof command !== "string" ||
          !command.trim() ||
          command.length > 65536 ||
          ctx.signal?.aborted ||
          typeof ctx.nativeInstanceId !== "string" ||
          !ctx.nativeInstanceId
        )
          return;
        const send = ctx.executionContext?.sendContext;
        const session =
          ctx.identity?.track === "agent"
            ? ctx.identity.conversationId
            : send?.conversationId || send?.localConversationId;
        const id = (value) =>
          typeof value === "string" &&
          value.length <= 200 &&
          value.length > 0 &&
          !/\s|[\u0000-\u001f\u007f]/.test(value)
            ? value
            : undefined;
        globalThis[binding](
          JSON.stringify({
            hook_event_name: "PreToolUse",
            tool_name: "Bash",
            tool_input: { command },
            session_id: id(session),
            tool_use_id: id(request.id),
          }),
        );
      } catch {
        // A missing observer never changes native permissions or execution.
      }
    },
  };
  lifecycle.push(hook);
  globalThis[slot] = {
    binding,
    dispose() {
      const index = lifecycle.indexOf(hook);
      if (index >= 0) lifecycle.splice(index, 1);
      if (globalThis[slot]?.dispose === this.dispose) delete globalThis[slot];
    },
  };
  return true;
}
