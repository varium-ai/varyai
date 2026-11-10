"use strict";
// Self-contained: executed in each user-launched native chat window.
function createStartingModelAdapter(req) {
  function legacyAgentType(type) {
    if (!req.m) return req(46623).KP(type);
    const ids = Object.keys(req.m).filter((id) => {
      const source = Function.prototype.toString.call(req.m[id]);
      return (
        /\bKP\s*:/.test(source) &&
        source.includes('"dev_agent"') &&
        source.includes('"builder_with_mcp"')
      );
    });
    if (ids.length !== 1) throw Error("starting_model_unavailable");
    return req(ids[0]).KP(type);
  }
  return {
    async select(nativeId) {
      try {
        const di = req(14284).$t.getInstance();
        if (!di.isInitialized()) throw Error();
        const store = di.resolve(Symbol.for("ModelStore")),
          domain = di.resolve(Symbol.for("ModelDomainService")),
          persistence = di.resolve(Symbol.for("ModelPersistenceService")),
          app = di.resolve(Symbol.for("ModelAppService")),
          port = req(8594).XT.tryResolve(Symbol.for("ModelSelectPort")),
          policy = port?.getHostContext?.()?.modelSelectionPolicy,
          models = req(58660),
          labelFor = req(87327).qR,
          targets = new Map();
        await port?.modelHostService?.ensureModelsLoaded?.();
        // Enumerate native Agent types; only modes exposing this exact custom model participate.
        for (const type of new Set(Object.values(req(73916).Eo))) {
          const label = labelFor(type, policy);
          if (targets.has(label)) continue;
          const candidates = store.modelsByAgentType
            .get(type, policy)
            .filter((row) => row.custom_model_id === nativeId);
          if (!candidates.length) continue;
          if (
            candidates.length !== 1 ||
            !models.UU(candidates[0]) ||
            port?.isModelSelectionRestricted?.(candidates[0])
          )
            throw Error();
          targets.set(label, {
            type,
            modelId: models.NR(candidates[0], label),
          });
        }
        if (!targets.size) throw Error();
        for (const [label, { type, modelId }] of targets) {
          const selection = { modelId, mode: models.VY.Manual };
          // No session ID: preserve explicit selections in existing conversations.
          if (
            port?.persistUserModelSelection &&
            (await port.persistUserModelSelection({
              agentType: type,
              selection,
            })) === false
          )
            throw Error();
          domain.setNewSessionSelection?.(label, undefined);
          domain.setRecentUserSelection(label, selection);
          await persistence.saveRecentUserSelection(
            domain.getRecentUserSelectionMap(),
          );
          app.notifyHostModelSelectionChange({
            agentType: type,
            modelId,
            mode: models.VY.Manual,
            modelChanged: true,
            modeChanged: true,
          });
          const actual = domain.getUserSelectedModel(undefined, type, policy);
          if (actual?.modelId !== modelId || actual?.mode !== models.VY.Manual)
            throw Error();
          const legacy = port?.legacyModelSelectionAdapter;
          if (legacy) {
            const legacyType = legacyAgentType(type),
              relation = legacy.sessionRelationStore;
            await relation.persistGlobalModelMap();
            await relation.persistGlobalModeMap();
            if (
              legacy.getSelectedModel(undefined, legacyType)
                ?.custom_model_id !== nativeId ||
              legacy.getSelectedMode(undefined, legacyType) !== models.VY.Manual
            )
              throw Error();
          }
        }
        return true;
      } catch {
        throw Error("starting_model_unavailable");
      }
    },
  };
}
module.exports = { createStartingModelAdapter };
