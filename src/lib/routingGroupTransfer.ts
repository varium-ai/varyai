import { z } from "zod";
import type {
  IntentRule,
  RoutingGroup,
  RoutingProviderOption,
} from "./api/intentRouting";

const strategy = z.enum([
  "smart",
  "ordered",
  "round_robin",
  "least_used",
  "pace",
  "free_first",
  "fastest",
  "weighted",
  "manual",
]);
const text = z.string().trim().min(1).max(200);
const ruleEfforts = ["low", "medium", "high", "xhigh", "max"] as const;
const folder = z
  .string()
  .trim()
  .min(1)
  .max(4096)
  .refine(
    (value) =>
      !/[\x00-\x1f\x7f]/.test(value) &&
      /^(?:\/|\\\\|~(?:[\/\\]|$)|[a-zA-Z]:[\/\\])/.test(value),
  );
const clock = z.string().regex(/^(?:[01]?\d|2[0-3]):[0-5]\d$/);
const timeWindow = z
  .object({
    from: z.union([clock, z.literal("")]),
    to: z.union([clock, z.literal("")]),
    days: z
      .array(z.enum(["mon", "tue", "wed", "thu", "fri", "sat", "sun"]))
      .max(7)
      .optional(),
    zone: z
      .string()
      .max(100)
      .refine((zone) => {
        if (!zone) return true;
        try {
          new Intl.DateTimeFormat("en", { timeZone: zone });
          return true;
        } catch {
          return false;
        }
      })
      .optional(),
  })
  .strict()
  .refine(
    (value) =>
      ((!!value.from && !!value.to && value.from !== value.to) ||
        (((!value.from && !value.to) || value.from === value.to) &&
          !!value.days?.length &&
          value.days.length < 7)) &&
      new Set(value.days).size === (value.days?.length ?? 0),
  );
const conditions = z
  .object({
    tokens: z.number().int().min(0).max(100_000_000).optional(),
    images: z.boolean().optional(),
    compact: z.boolean().optional(),
    effort: z.enum(["", "on", ...ruleEfforts]).optional(),
    agents: z.array(text).max(32).optional(),
    time: timeWindow.nullable().optional(),
    folders: z.array(folder).max(32).optional(),
    exceptFolders: z.array(folder).max(32).optional(),
  })
  .strict()
  .refine(
    (value) =>
      !!(
        value.tokens ||
        value.images ||
        value.compact ||
        value.effort ||
        value.agents?.length ||
        value.time ||
        value.folders?.length ||
        value.exceptFolders?.length
      ),
  );

export function validRoutingRule(rule: IntentRule) {
  const facts = rule.conditions;
  return (
    !!rule.name.trim() &&
    !!rule.targetMemberId &&
    (!!rule.description.trim() ||
      (!!facts && conditions.safeParse(facts).success)) &&
    (!facts || conditions.safeParse(facts).success) &&
    (!rule.pause ||
      (!rule.description.trim() &&
        !rule.useEffort &&
        !rule.fallbackMemberIds?.length &&
        !facts?.tokens &&
        !facts?.images &&
        !facts?.effort &&
        !facts?.compact))
  );
}
export const routingLevels = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;
const levels = z
  .array(z.enum(routingLevels))
  .max(routingLevels.length)
  .refine((items) =>
    items.every(
      (item, index) =>
        index === 0 ||
        routingLevels.indexOf(items[index - 1]) < routingLevels.indexOf(item),
    ),
  )
  .optional();
export function validRoutingPattern(pattern: string) {
  if (!pattern || pattern.length > 1024 || /[\x00-\x1f\x7f]/.test(pattern))
    return false;
  if (!pattern.includes("*") && !pattern.startsWith("re:")) return false;
  if (!pattern.startsWith("re:")) return true;
  try {
    let expression = pattern.slice(3);
    if (!expression.trim()) return false;
    const flags = new Set(["i"]);
    const leading = /^\(\?([imsU]*)(?:-([imsU]*))?\)/.exec(expression);
    if (leading) {
      if (!leading[1] && !leading[2]) return false;
      for (const flag of leading[1]) if (flag !== "U") flags.add(flag);
      for (const flag of leading[2] ?? "") flags.delete(flag);
      expression = expression.slice(leading[0].length);
    }
    new RegExp(`^(?:${expression})$`, [...flags].join(""));
    return true;
  } catch {
    return false;
  }
}
const patterns = z
  .array(z.string().refine(validRoutingPattern))
  .max(32)
  .optional();
const member = z
  .object({
    id: text,
    providerId: text,
    modelId: text,
    weight: z.number().int().min(1).max(1000).optional(),
    free: z.boolean().nullable().optional(),
    effort: z.enum(routingLevels).nullable().optional(),
  })
  .strict();
const nativeGroup = z
  .object({
    id: text,
    name: z.string().trim().min(1).max(80),
    revision: z.number().int().nonnegative(),
    client: z.enum(["global", "codex"]),
    enabled: z.boolean(),
    members: z.array(member).max(256),
    defaultMemberId: z.string().max(200),
    fallbackMemberIds: z.array(text).max(256),
    intentRecognitionEnabled: z.boolean(),
    classifierTarget: z
      .object({ providerId: text, modelId: text })
      .strict()
      .nullable(),
    classificationBudgetMs: z.number().int().min(500).max(5000),
    rules: z
      .array(
        z
          .object({
            id: text,
            name: text,
            description: z.string().max(4000),
            enabled: z.boolean(),
            targetMemberId: text,
            fallbackMemberIds: z.array(text).max(256).optional(),
            conditions: conditions.nullable().optional(),
            useEffort: z.enum(["", ...ruleEfforts]).optional(),
            pause: z.boolean().optional(),
          })
          .strict(),
      )
      .max(16),
    routing: strategy.optional(),
    autoGenerated: z.boolean().optional(),
    manualMemberId: text.optional(),
    modelAlias: text.optional(),
    contextWindow: z
      .number()
      .int()
      .min(-1)
      .max(Number.MAX_SAFE_INTEGER)
      .optional(),
    affinity: z.enum(["", "session", "turn", "off", "strict"]).optional(),
    firstToken: z.number().int().min(0).max(3600).optional(),
    patience: z.number().int().min(-1).max(240).optional(),
    off: z.array(text).max(256).optional(),
    match: patterns,
    unlists: z.boolean().optional(),
    sink: z.boolean().optional(),
    levels,
  })
  .strict();
const sharedGroupSchema = z
  .object({
    id: text.optional(),
    name: z.string().trim().min(1).max(80),
    members: z.array(text).max(256),
    routing: z.string().optional(),
    weights: z.record(z.string(), z.number().int().min(1).max(1000)).optional(),
    pick: text.optional(),
    context: z.number().int().min(-1).max(Number.MAX_SAFE_INTEGER).optional(),
    disabled: z.boolean().optional(),
    affinity: z.enum(["", "session", "turn", "off", "strict"]).optional(),
    firstToken: z.number().int().min(0).max(3600).optional(),
    patience: z.number().int().min(-1).max(240).optional(),
    off: z.array(text).max(256).optional(),
    match: patterns,
    unlists: z.boolean().optional(),
    sink: z.boolean().optional(),
    levels,
    rules: z
      .array(
        z
          .object({
            use: text,
            more: z.array(text).max(256).optional(),
            useEffort: z.enum(ruleEfforts).optional(),
            tokens: z.number().int().min(0).max(100_000_000).optional(),
            images: z.boolean().optional(),
            effort: z.enum(["on", ...ruleEfforts]).optional(),
            agents: z.array(text).max(32).optional(),
            compact: z.boolean().optional(),
            intent: z.string().trim().min(1).max(200).optional(),
            time: timeWindow.optional(),
            folders: z.array(folder).max(32).optional(),
            exceptFolders: z.array(folder).max(32).optional(),
            pause: z.boolean().optional(),
          })
          .strict(),
      )
      .max(16)
      .optional(),
    classifier: text.optional(),
  })
  .strict();

// The reference implementation's portable envelope deliberately excludes machine-local folder rules.
export function shareRoutingGroup(group: RoutingGroup): string {
  const refs = new Map(
    group.members.map((member) => [
      member.id,
      `${member.providerId}/${member.modelId}${member.effort ? `:${member.effort}` : ""}`,
    ]),
  );
  const routes: Record<NonNullable<RoutingGroup["routing"]>, string> = {
    smart: "",
    ordered: "order",
    round_robin: "rotate",
    least_used: "usage",
    pace: "pace",
    free_first: "free",
    fastest: "fast",
    weighted: "weight",
    manual: "manual",
  };
  const rules = group.rules
    .filter(
      (rule) =>
        rule.enabled &&
        !rule.conditions?.folders?.length &&
        !rule.conditions?.exceptFolders?.length,
    )
    .map((rule) => {
      const use = refs.get(rule.targetMemberId);
      if (!use) throw new Error("invalid");
      const conditions = rule.conditions;
      const facts = {
        ...(conditions?.tokens ? { tokens: conditions.tokens } : {}),
        ...(conditions?.images ? { images: true } : {}),
        ...(conditions?.effort ? { effort: conditions.effort } : {}),
        ...(conditions?.agents?.length ? { agents: conditions.agents } : {}),
        ...(conditions?.compact ? { compact: true } : {}),
        ...(conditions?.time ? { time: conditions.time } : {}),
      };
      return {
        ...facts,
        ...(rule.description.trim() ? { intent: rule.description.trim() } : {}),
        use,
        ...(rule.fallbackMemberIds?.length
          ? {
              more: rule.fallbackMemberIds.map((id) => {
                const ref = refs.get(id);
                if (!ref) throw new Error("invalid");
                return ref;
              }),
            }
          : {}),
        ...(rule.useEffort ? { useEffort: rule.useEffort } : {}),
        ...(rule.pause ? { pause: true } : {}),
      };
    });
  if (
    rules.some((rule) => "intent" in rule) &&
    (!group.intentRecognitionEnabled || !group.classifierTarget)
  )
    throw new Error("invalid");
  return JSON.stringify(
    {
      magpie: "routing-group",
      version: 1,
      group: {
        id: group.id,
        name: group.name,
        members: [...refs.values()],
        routing: routes[group.routing ?? "ordered"],
        match: group.match,
        affinity: group.affinity,
        sink: group.sink,
        firstToken: group.firstToken,
        patience: group.patience,
        context: group.contextWindow,
        levels: group.levels,
        unlists: group.unlists,
        off: group.off?.flatMap((id) => refs.get(id) ?? []),
        pick: group.manualMemberId ? refs.get(group.manualMemberId) : undefined,
        weights: Object.fromEntries(
          group.members
            .filter((member) => (member.weight ?? 1) > 1)
            .map((member) => [refs.get(member.id)!, member.weight]),
        ),
        classifier:
          group.intentRecognitionEnabled && group.classifierTarget
            ? `${group.classifierTarget.providerId}/${group.classifierTarget.modelId}`
            : undefined,
        rules,
      },
    },
    null,
    2,
  );
}

export interface RoutingImportResult {
  group: RoutingGroup;
  missing: string[];
  instead: string[];
  omittedRules: number;
}

// Adapted from the reference implementation catalog.SameModel; matching never rewrites a wire model ID.
function sameModel(value: string) {
  let key = value
    .trim()
    .toLowerCase()
    .split("/")
    .at(-1)!
    .replace(/[\s_-]+/gu, "-")
    .replace(/^-|-$/g, "")
    .replace(/(?<=\d)[.p](?=\d)/g, "-");
  const date = /[-@](\d+)$/.exec(key);
  if (
    date &&
    ((date[1].length === 8 && date[1].startsWith("20")) ||
      /^(?:2[3-9]|3[0-9])(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])$/.test(
        date[1],
      ))
  ) {
    key = key.slice(0, date.index);
  }
  return key;
}

export function importSharedRoutingGroup(
  input: string,
  providers: RoutingProviderOption[],
): RoutingImportResult {
  if (input.length > 65536) throw new Error("invalid");
  const start = input.indexOf("{");
  const end = input.lastIndexOf("}");
  if (start < 0 || end < start) throw new Error("invalid");
  const parsed: unknown = JSON.parse(input.slice(start, end + 1));
  const envelope = z
    .object({
      magpie: z.literal("routing-group"),
      version: z.literal(1),
      group: sharedGroupSchema,
    })
    .strict()
    .safeParse(parsed);
  if (!envelope.success) {
    if (typeof parsed === "object" && parsed && "magpie" in parsed)
      throw new Error("invalid");
    return {
      group: importRoutingGroup(JSON.stringify(parsed), providers),
      missing: [],
      instead: [],
      omittedRules: 0,
    };
  }
  const original = envelope.data.group;
  const missing: string[] = [];
  const instead: string[] = [];
  const placed = new Map<string, string | null>();
  function place(value: string): string | null {
    if (placed.has(value)) return placed.get(value)!;
    const canonical = value.trim().replace(/^magpie\//, "");
    let found: string | null = null;
    if (!canonical.startsWith("group/")) {
      const known = providers
        .filter((provider) => canonical.startsWith(`${provider.id}/`))
        .sort((a, b) => b.id.length - a.id.length)[0];
      let model = canonical.slice(
        known ? known.id.length + 1 : canonical.indexOf("/") + 1,
      );
      let effort = "";
      if (
        !providers.some((provider) =>
          provider.models?.some((entry) => entry.id === model),
        )
      ) {
        const colon = model.lastIndexOf(":");
        const suffix = model.slice(colon + 1);
        if (colon > 0 && routingLevels.some((level) => level === suffix)) {
          effort = `:${suffix}`;
          model = model.slice(0, colon);
        }
      }
      const exact = known?.models?.find((entry) => entry.id === model);
      const local =
        exact && known
          ? { provider: known, model: exact }
          : providers
              .flatMap((provider) =>
                (provider.models ?? []).map((model) => ({ provider, model })),
              )
              .find((entry) => sameModel(entry.model.id) === sameModel(model));
      if (canonical.includes("/") && local)
        found = `${local.provider.id}/${local.model.id}${effort}`;
    }
    placed.set(value, found);
    if (!found) missing.push(value);
    else if (found !== canonical) instead.push(`${value} → ${found}`);
    return found;
  }
  const members = [
    ...new Set(original.members.flatMap((member) => place(member) ?? [])),
  ];
  if (!members.length && !original.match?.length) throw new Error("invalid");
  const classifier = original.classifier ? place(original.classifier) : null;
  let omittedRules = 0;
  const rules = (original.rules ?? []).flatMap((rule) => {
    const uses = [
      ...new Set(
        [rule.use, ...(rule.more ?? [])].flatMap((use) => {
          const ref = place(use);
          return ref && members.includes(ref) ? [ref] : [];
        }),
      ),
    ];
    if (
      !uses.length ||
      (rule.intent && !classifier) ||
      rule.folders?.length ||
      rule.exceptFolders?.length
    ) {
      omittedRules += 1;
      return [];
    }
    const { use: _use, more: _more, ...rest } = rule;
    return [{ ...rest, use: uses[0], more: uses.slice(1) }];
  });
  let off = [
    ...new Set((original.off ?? []).flatMap((ref) => placed.get(ref) ?? [])),
  ];
  if (original.routing !== "manual" && off.length === members.length) off = [];
  const weights: Record<string, number> = {};
  for (const [ref, weight] of Object.entries(original.weights ?? {})) {
    const local = placed.get(ref);
    if (local) weights[local] = weight;
  }
  const group = importRoutingGroup(
    JSON.stringify({
      ...original,
      members,
      rules,
      off,
      weights,
      disabled: false,
      pick: original.pick
        ? (placed.get(original.pick) ?? undefined)
        : undefined,
      classifier: classifier ?? undefined,
    }),
    providers,
  );
  return { group, missing, instead, omittedRules };
}

export function blankRoutingGroup(): RoutingGroup {
  return {
    id: crypto.randomUUID(),
    name: "",
    revision: 0,
    client: "global",
    enabled: true,
    members: [],
    defaultMemberId: "",
    fallbackMemberIds: [],
    intentRecognitionEnabled: false,
    classifierTarget: null,
    classificationBudgetMs: 2000,
    rules: [],
    routing: "smart",
    patience: 0,
  };
}

export function importRoutingGroup(
  input: string,
  providers: RoutingProviderOption[],
): RoutingGroup {
  if (input.length > 65536) throw new Error("invalid");
  const parsed: unknown = JSON.parse(input);
  const wrapped = z
    .object({
      format: z.literal("vary-routing-group"),
      version: z.literal(1),
      group: nativeGroup,
    })
    .strict()
    .safeParse(parsed);
  const native = wrapped.success
    ? { success: true as const, data: wrapped.data.group }
    : nativeGroup.safeParse(parsed);
  let group: RoutingGroup;
  if (native.success) {
    const original = native.data;
    if (!original.rules.every(validRoutingRule)) throw new Error("invalid");
    const ids = new Map(
      original.members.map((m) => [m.id, crypto.randomUUID()]),
    );
    if (
      ids.size !== original.members.length ||
      (!ids.has(original.defaultMemberId) &&
        !(
          original.members.length === 0 &&
          original.match?.length &&
          !original.defaultMemberId
        )) ||
      original.fallbackMemberIds.some((id) => !ids.has(id)) ||
      original.rules.some((rule) => !ids.has(rule.targetMemberId)) ||
      original.rules.some(
        (rule) =>
          rule.fallbackMemberIds?.some(
            (id) => !ids.has(id) || id === rule.targetMemberId,
          ) ||
          new Set(rule.fallbackMemberIds).size !==
            (rule.fallbackMemberIds?.length ?? 0),
      ) ||
      original.off?.some(
        (id) =>
          !ids.has(id) &&
          !(original.match?.length && /^match-[a-f0-9]{64}$/.test(id)),
      ) ||
      new Set(original.off).size !== (original.off?.length ?? 0) ||
      (original.manualMemberId &&
        !ids.has(original.manualMemberId) &&
        !(
          original.match?.length &&
          /^match-[a-f0-9]{64}$/.test(original.manualMemberId)
        ))
    )
      throw new Error("invalid");
    group = {
      ...original,
      id: crypto.randomUUID(),
      revision: 0,
      client: "global",
      autoGenerated: false,
      modelAlias: undefined,
      off: original.off?.map((id) => ids.get(id) ?? id),
      members: original.members.map((m) => ({ ...m, id: ids.get(m.id)! })),
      defaultMemberId: ids.get(original.defaultMemberId) ?? "",
      fallbackMemberIds: original.fallbackMemberIds.map((id) => ids.get(id)!),
      manualMemberId: original.manualMemberId
        ? (ids.get(original.manualMemberId) ?? original.manualMemberId)
        : undefined,
      rules: original.rules.map((rule) => ({
        ...rule,
        id: crypto.randomUUID(),
        targetMemberId: ids.get(rule.targetMemberId)!,
        fallbackMemberIds: rule.fallbackMemberIds?.map((id) => ids.get(id)!),
      })),
    };
  } else {
    const original = sharedGroupSchema.parse(parsed);
    const strategies: Record<string, NonNullable<RoutingGroup["routing"]>> = {
      "": "smart",
      order: "ordered",
      rotate: "round_robin",
      usage: "least_used",
      free: "free_first",
      fast: "fastest",
      weight: "weighted",
      ...Object.fromEntries(strategy.options.map((s) => [s, s])),
    };
    const routing = strategies[original.routing ?? ""];
    if (!routing) throw new Error("invalid");
    const members = original.members.map((value) => {
      const provider = providers
        .filter((provider) => value.startsWith(`${provider.id}/`))
        .sort((a, b) => b.id.length - a.id.length)[0];
      if (!provider || value.startsWith("group/")) throw new Error("invalid");
      let modelId = value.slice(provider.id.length + 1);
      let effort: string | undefined;
      if (!provider.models?.some((model) => model.id === modelId)) {
        const colon = modelId.lastIndexOf(":");
        const suffix = modelId.slice(colon + 1);
        if (colon > 0 && routingLevels.some((level) => level === suffix)) {
          effort = suffix;
          modelId = modelId.slice(0, colon);
        }
      }
      if (!provider.models?.some((model) => model.id === modelId))
        throw new Error("invalid");
      return {
        id: crypto.randomUUID(),
        providerId: provider.id,
        modelId,
        effort,
        weight: original.weights?.[value] ?? 1,
      };
    });
    if (original.pick && !original.members.includes(original.pick))
      throw new Error("invalid");
    if (
      original.off?.some((id) => !original.members.includes(id)) ||
      new Set(original.off).size !== (original.off?.length ?? 0)
    )
      throw new Error("invalid");
    if (
      Object.keys(original.weights ?? {}).some(
        (key) => !original.members.includes(key),
      )
    )
      throw new Error("invalid");
    group = {
      ...blankRoutingGroup(),
      name: original.name,
      enabled: !original.disabled,
      routing,
      members,
      defaultMemberId: members[0]?.id ?? "",
      fallbackMemberIds: members.slice(1).map((m) => m.id),
      manualMemberId: original.pick
        ? members[original.members.indexOf(original.pick)].id
        : undefined,
      contextWindow: original.context,
      firstToken: original.firstToken ?? 0,
      patience: original.patience ?? 0,
      affinity: original.affinity,
      match: original.match,
      unlists: original.unlists,
      sink: original.sink,
      levels: original.levels,
      off: original.off?.map((id) => members[original.members.indexOf(id)].id),
    };
    group.rules = (original.rules ?? []).map((rule, index) => {
      const uses = [rule.use, ...(rule.more ?? [])];
      if (
        uses.some((use) => !original.members.includes(use)) ||
        new Set(uses).size !== uses.length
      )
        throw new Error("invalid");
      const facts = {
        tokens: rule.tokens,
        images: rule.images,
        effort: rule.effort,
        agents: rule.agents,
        compact: rule.compact,
        time: rule.time,
        folders: rule.folders,
        exceptFolders: rule.exceptFolders,
      };
      const checked = conditions.safeParse(facts);
      if (!rule.intent && !checked.success) throw new Error("invalid");
      if (
        rule.pause &&
        (rule.more?.length ||
          rule.useEffort ||
          rule.tokens ||
          rule.images ||
          rule.effort ||
          rule.intent ||
          rule.compact)
      )
        throw new Error("invalid");
      return {
        id: crypto.randomUUID(),
        name: rule.intent || `Rule ${index + 1}`,
        description: rule.intent ?? "",
        enabled: true,
        targetMemberId: members[original.members.indexOf(rule.use)].id,
        fallbackMemberIds: (rule.more ?? []).map(
          (use) => members[original.members.indexOf(use)].id,
        ),
        useEffort: rule.useEffort,
        pause: rule.pause,
        conditions: checked.success ? checked.data : undefined,
      };
    });
    if (original.classifier) {
      const provider = providers
        .filter((provider) =>
          original.classifier!.startsWith(`${provider.id}/`),
        )
        .sort((a, b) => b.id.length - a.id.length)[0];
      if (!provider?.classifierEligible) throw new Error("invalid");
      group.classifierTarget = {
        providerId: provider.id,
        modelId: original.classifier.slice(provider.id.length + 1),
      };
      group.intentRecognitionEnabled = true;
    }
    if (group.rules.some((rule) => rule.description) && !group.classifierTarget)
      throw new Error("invalid");
  }
  if (group.members.length === 0 && !group.match?.length)
    throw new Error("invalid");
  for (const target of [
    ...group.members,
    ...(group.classifierTarget ? [group.classifierTarget] : []),
  ]) {
    if (
      !providers.some(
        (provider) =>
          provider.id === target.providerId &&
          provider.models?.some((model) => model.id === target.modelId),
      )
    )
      throw new Error("invalid");
  }
  if (
    new Set(
      group.members.map((m) =>
        JSON.stringify([m.providerId, m.modelId, m.effort ?? null]),
      ),
    ).size !== group.members.length
  )
    throw new Error("invalid");
  return group;
}

export interface RoutingTemplate {
  key: "templateSmart" | "templateThrifty" | "templateSteady";
  need?: "provider" | "providers" | "cheap" | "classifier";
  of?: string;
  group: RoutingGroup;
}

// The reference implementation's template ranking and rules, not preset model names. MIT attribution:
// third_party/routing-reference/LICENSE, internal/provider/grouptemplate.go.
export function routingTemplates(
  providers: RoutingProviderOption[],
): RoutingTemplate[] {
  const nonChat =
    /embed|rerank|image|imagen|dall-e|tts|whisper|audio|realtime|transcribe|moderation|ocr|veo|sora|speech|research|guard|computer-use/;
  const smallWords = new Set([
    "mini",
    "nano",
    "flash",
    "haiku",
    "lite",
    "small",
    "air",
    "turbo",
    "fast",
    "instant",
    "tiny",
    "highspeed",
  ]);
  const tinyWords = new Set(["nano", "lite", "tiny"]);
  const available = providers
    .filter((provider) => provider.taskEligible)
    .flatMap((provider) =>
      (provider.models ?? []).map((model) => {
        const words = model.id.toLowerCase().split(/[^a-z0-9.]+/);
        const sizes = words
          .map((word) => /^(\d+(?:\.\d+)?)b$/.exec(word))
          .filter((match) => match !== null)
          .map((match) => Number(match[1]));
        return {
          provider,
          model,
          out:
            typeof model.outputPrice === "number" &&
            Number.isFinite(model.outputPrice) &&
            model.outputPrice >= 0
              ? model.outputPrice
              : 0,
          maker: !!model.maker,
          small:
            words.some((word) => smallWords.has(word)) ||
            sizes.some((size) => size <= 72),
          tiny:
            words.some((word) => tinyWords.has(word)) ||
            sizes.some((size) => size <= 15),
        };
      }),
    )
    .filter(({ model }) => !nonChat.test(model.id.toLowerCase()));
  const newer = (
    a: (typeof available)[number],
    b: (typeof available)[number],
  ) =>
    (b.model.released ?? "").localeCompare(a.model.released ?? "") ||
    b.out - a.out;
  const strongest = available
    .filter((candidate) => candidate.out <= 100)
    .sort(
      (a, b) =>
        Number(b.maker) - Number(a.maker) ||
        Number(b.out >= 10) - Number(a.out >= 10) ||
        Number(a.small) - Number(b.small) ||
        newer(a, b),
    );
  return (["templateSmart", "templateThrifty", "templateSteady"] as const).map(
    (key) => {
      const group = { ...blankRoutingGroup(), routing: "ordered" as const };
      const result: RoutingTemplate = { key, group };
      const best = strongest[0];
      if (!best) return { ...result, need: "provider" };
      const cheap = available
        .filter(
          (candidate) =>
            candidate !== best &&
            (best.out > 0 && candidate.out > 0
              ? candidate.out <= best.out / 4
              : candidate.small && !best.small),
        )
        .sort(
          (a, b) =>
            Number(b.maker) - Number(a.maker) ||
            Number(a.tiny) - Number(b.tiny) ||
            newer(a, b),
        );
      let candidates: typeof available;
      if (key === "templateSteady") {
        const seen = new Set<string>();
        candidates = strongest
          .filter(({ provider }) => {
            if (seen.has(provider.id)) return false;
            seen.add(provider.id);
            return true;
          })
          .slice(0, 3);
        if (candidates.length < 2)
          return { ...result, need: "providers", of: best.provider.name };
      } else {
        const low = cheap[0];
        if (!low)
          return {
            ...result,
            need: "cheap",
            of: best.model.name || best.model.id,
          };
        if (key === "templateSmart") {
          candidates = [best, low];
          const classifier = available
            .filter(
              (candidate) =>
                candidate.out > 0 &&
                candidate.model.reasoning !== true &&
                !candidate.tiny &&
                candidate.provider.classifierEligible,
            )
            .sort((a, b) => a.out - b.out)[0];
          const chosen =
            classifier && classifier.out <= low.out ? classifier : low;
          if (!chosen.provider.classifierEligible)
            return { ...result, need: "classifier" };
          group.classifierTarget = {
            providerId: chosen.provider.id,
            modelId: chosen.model.id,
          };
          group.intentRecognitionEnabled = true;
        } else {
          const bare = (model: string) =>
            model.slice(model.lastIndexOf("/") + 1).toLowerCase();
          const other =
            cheap.find(
              (candidate) =>
                candidate.provider.id !== low.provider.id &&
                bare(candidate.model.id) !== bare(low.model.id),
            ) ??
            cheap.find(
              (candidate) => candidate.provider.id !== low.provider.id,
            );
          candidates = [low, ...(other ? [other] : []), best];
        }
      }
      group.members = candidates.map(({ provider, model }) => ({
        id: crypto.randomUUID(),
        providerId: provider.id,
        modelId: model.id,
      }));
      group.defaultMemberId = group.members[0].id;
      group.fallbackMemberIds = group.members
        .slice(1)
        .map((member) => member.id);
      const addRule = (
        target: number,
        name: string,
        conditions?: IntentRule["conditions"],
        description = "",
      ) => {
        group.rules.push({
          id: crypto.randomUUID(),
          name,
          description,
          enabled: true,
          targetMemberId: group.members[target].id,
          ...(conditions ? { conditions } : {}),
        });
      };
      if (key === "templateSmart") {
        addRule(1, "Compaction", { compact: true });
        if (best.model.images && !cheap[0].model.images)
          addRule(0, "Images", { images: true });
        addRule(
          0,
          "Hard task",
          undefined,
          "a hard task: designing, debugging, a change across several files, or planning",
        );
        addRule(
          1,
          "Easy task",
          undefined,
          "an easy task: a quick question, a small edit, a lookup, or running a command",
        );
      } else if (key === "templateThrifty") {
        addRule(group.members.length - 1, "High reasoning", { effort: "high" });
        if (best.model.images && !cheap[0].model.images)
          addRule(group.members.length - 1, "Images", { images: true });
      }
      return result;
    },
  );
}
