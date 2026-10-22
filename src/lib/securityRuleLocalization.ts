import type { TFunction } from "i18next";

const BUILTIN_RULE_IDS = new Set([
  "rsa_ssh_private_key",
  "github_token",
  "aws_access_key",
  "openai_api_key",
  "anthropic_api_key",
  "db_connection_uri",
  "rfc1918_private_ip",
  "catalog:gitlab-pat",
  "catalog:stripe-key",
  "catalog:npm-token",
  "catalog:pypi-token",
  "catalog:slack-bot-token",
  "catalog:bedrock-key",
  "catalog:google-api-key",
  "catalog:sendgrid-token",
  "catalog:huggingface-token",
  "catalog:digitalocean-pat",
  "catalog:notion-token",
  "catalog:linear-key",
  "catalog:shopify-token",
  "catalog:cloudflare-origin-key",
  "catalog:openrouter-key",
  "catalog:groq-key",
  "catalog:perplexity-key",
  "catalog:generic-sk-key-notice",
  "catalog:login-url-password",
  "catalog:password-assignment",
  "catalog:password-field",
  "chinese_id_card",
  "chinese_phone_number",
]);

export function getBuiltinRuleText(
  t: TFunction,
  ruleId: string,
  field: "name" | "description",
  fallback: string,
) {
  if (!BUILTIN_RULE_IDS.has(ruleId)) return fallback;
  const translationKey = ruleId.replace(/:/g, "_");
  return t(`security.builtinRules.${translationKey}.${field}`, {
    defaultValue: fallback,
  });
}

export function getBuiltinRuleName(
  t: TFunction,
  ruleId: string,
  fallback: string,
) {
  return getBuiltinRuleText(t, ruleId, "name", fallback);
}

export function getBuiltinRuleDescription(
  t: TFunction,
  ruleId: string,
  fallback: string,
) {
  return getBuiltinRuleText(t, ruleId, "description", fallback);
}
