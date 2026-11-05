export const subscriptionProviders = [
  {
    id: "codex",
    name: "ChatGPT / Codex",
    icon: "openai",
    aliases: "chatgpt codex openai",
  },
  {
    id: "github_copilot",
    name: "GitHub Copilot",
    icon: "github",
    aliases: "github copilot",
  },
  { id: "xai_oauth", name: "Grok", icon: "xai", aliases: "xai grok supergrok" },
] as const;
export type SubscriptionProviderId =
  (typeof subscriptionProviders)[number]["id"];
