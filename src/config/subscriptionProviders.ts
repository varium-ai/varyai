export const subscriptionProviders = [
  {
    id: "claude",
    name: "Claude",
    icon: "claude",
    aliases: "anthropic claude code",
  },
  {
    id: "codex",
    name: "ChatGPT / Codex",
    icon: "openai",
    aliases: "chatgpt codex openai",
  },
  {
    id: "chatgpt_api",
    name: "ChatGPT API",
    icon: "openai",
    aliases: "openai sign in with chatgpt siwc",
  },
  {
    id: "github_copilot",
    name: "GitHub Copilot",
    icon: "github",
    aliases: "github copilot",
  },
  {
    id: "gemini_cli",
    name: "Gemini CLI",
    icon: "geminicli",
    aliases: "google gemini cli code assist",
  },
  {
    id: "antigravity",
    name: "Antigravity",
    icon: "antigravity",
    aliases: "google antigravity",
  },
  { id: "xai_oauth", name: "Grok", icon: "xai", aliases: "xai grok supergrok" },
] as const;
export type SubscriptionProviderId =
  (typeof subscriptionProviders)[number]["id"];
