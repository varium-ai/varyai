// Vendor sources and ranking limits: docs/guides/routing-model-naming.md.
// These hints order recommendations only; they never describe runtime capabilities.
export type CapabilityNaming = {
  tier: number;
  family?: string;
  preference: number;
  version: string;
};

const lightweight = 0;
const standard = 1;
const flagship = 2;
const neutral: CapabilityNaming = {
  tier: standard,
  preference: 0,
  version: "",
};

function known(
  family: string,
  version: string | undefined,
  tier = standard,
  preference = 0,
): CapabilityNaming {
  return {
    tier,
    family: version ? family : undefined,
    preference,
    version: version
      ? `${version.replaceAll("-", ".")}.0.0`.split(".").slice(0, 3).join(".")
      : "",
  };
}

export function routingCapabilityNaming(modelId: string): CapabilityNaming {
  const name = (modelId.toLowerCase().split("/").at(-1) ?? "").split(/[:@]/)[0];
  const model = name.replace(
    /^(?:(?:global|us|eu|apac|jp|au)\.)?anthropic\.(?=claude-)/,
    "",
  );

  const claude =
    /^claude-(fable|opus|sonnet|haiku)-(\d{1,2}(?:[.-]\d{1,2})?)(?=-|$)/.exec(
      model,
    );
  const legacyClaude =
    /^claude-(\d{1,2}(?:[.-]\d{1,2})?)-(opus|sonnet|haiku)(?=-|$)/.exec(model);
  if (claude || legacyClaude) {
    const edition = claude?.[1] ?? legacyClaude?.[2];
    const version = claude?.[2] ?? legacyClaude?.[1];
    if (edition === "fable" || edition === "opus")
      return known("claude", version, flagship, edition === "fable" ? 1 : 0);
    return known(
      "claude",
      version,
      edition === "haiku" ? lightweight : standard,
    );
  }

  // A version has at most three numeric components. Dated snapshots and
  // transport/preview suffixes are not version components.
  const numbered =
    /^(gpt-|gemini-|deepseek-v?|qwen-?|glm-|kimi-k|grok-|minimax-m|(?:doubao-|dola-)?seed-|mimo-v)(\d{1,2}(?:[.-]\d{1,2}){0,2})(?:-([a-z][a-z0-9]*))?(?=-|$)/.exec(
      model,
    );
  if (numbered) {
    const [, prefix, version, edition = ""] = numbered;
    const major = Number(version.split(/[.-]/)[0]);
    switch (prefix) {
      case "gpt-":
        if (
          edition === "pro" ||
          edition === "astra" ||
          (version === "5.6" && edition === "sol")
        )
          return known("gpt", version, flagship);
        if (edition === "luna" || edition === "mini" || edition === "nano")
          return known("gpt", version, lightweight, edition === "mini" ? 1 : 0);
        if (edition === "sol" || edition === "terra" || !edition)
          return known("gpt", version, standard, edition === "sol" ? 1 : 0);
        break;
      case "gemini-":
        if (edition === "pro" || edition === "argon" || edition === "carbon")
          return known("gemini", version, flagship);
        if (edition === "neon") return known("gemini", version);
        if (edition === "hydrogen" || edition === "flash") {
          const lite = /^gemini-[\d.-]+-flash-lite(?:-|$)/.test(model);
          return known("gemini", version, lightweight, lite ? 0 : 1);
        }
        break;
      case "deepseek-":
      case "deepseek-v":
        if (edition === "pro") return known("deepseek", version, flagship);
        if (edition === "flash") return known("deepseek", version, lightweight);
        if (!edition) return known("deepseek", version);
        break;
      case "qwen":
      case "qwen-":
        if (edition === "max") return known("qwen", version, flagship);
        if (edition === "plus") return known("qwen", version);
        if (edition === "flash" || edition === "turbo")
          return known("qwen", version, lightweight);
        break;
      case "glm-":
        if (/^(flashx?|airx?)$/.test(edition))
          return known("glm", version, lightweight);
        if (edition === "turbo") return known("glm-turbo", version);
        if (!edition)
          return known("glm", version, major === 5 ? flagship : standard);
        break;
      case "kimi-k":
        if (!edition || edition === "pro" || edition === "highspeed")
          return known(
            "kimi-k",
            version,
            major === 3 || edition === "pro" ? flagship : standard,
          );
        if (/^(code|thinking|turbo|preview)$/.test(edition))
          return known(`kimi-k-${edition}`, version);
        break;
      case "grok-":
        // The release notes place 4.20 before 4.3 through 4.7.
        if (
          !edition ||
          /^(fast|reasoning|non|multi|latest|mini)$/.test(edition)
        )
          return known(
            "grok",
            version === "4.20" ? "4.2" : version,
            edition === "mini"
              ? lightweight
              : /^4[.-][5-7]$/.test(version)
                ? flagship
                : standard,
          );
        break;
      case "minimax-m":
        // M3 and M3.1 Flash are both published as frontier models. Compare
        // versions within a line, not between them, and ignore serving speed.
        if (!edition || edition === "flash" || edition === "highspeed")
          return known(
            edition === "flash" ? "minimax-flash" : "minimax",
            version,
            major === 3 ? flagship : standard,
          );
        break;
      case "doubao-seed-":
      case "dola-seed-":
      case "seed-":
        if (edition === "pro") return known("seed", version, flagship);
        if (edition === "lite" || edition === "turbo")
          return known("seed", version);
        if (edition === "mini") return known("seed", version, lightweight);
        break;
      case "mimo-v":
        if (edition === "pro") return known("mimo", version, flagship);
        if (edition === "flash") return known("mimo", version, lightweight);
        if (!edition) return known("mimo", version);
        break;
    }
  }

  if (/^deepseek-flash(?:-|$)/.test(model))
    return known("deepseek", undefined, lightweight);
  if (/^qwen-max(?:-|$)/.test(model)) return known("qwen", undefined, flagship);
  if (/^qwen-(flash|turbo)(?:-|$)/.test(model))
    return known("qwen", undefined, lightweight);

  const mistral =
    /^mistral-(large|medium|small)(?:-(\d{4}|\d{1,2}(?:[.-]\d{1,2})?))?(?=-|$)/.exec(
      model,
    );
  if (mistral) {
    const [, edition, version] = mistral;
    const tier =
      edition === "large"
        ? flagship
        : edition === "small"
          ? lightweight
          : standard;
    // Keep YYMM names in a separate comparison pool from numbered generations.
    return known(
      version?.length === 4 && /^\d+$/.test(version)
        ? "mistral-date"
        : "mistral",
      version,
      tier,
    );
  }
  if (/^ministral-(?:\d|latest)/.test(model))
    return known("ministral", undefined, lightweight);
  return neutral;
}
