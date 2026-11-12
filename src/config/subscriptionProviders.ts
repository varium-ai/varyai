export const subscriptionProviders = [
  {
    id: "cursor",
    website: "https://cursor.com",
    name: "Cursor",
    icon: "cursor",
    aliases: "cursor subscription agent",
  },
  {
    id: "claude",
    website: "https://claude.ai",
    name: "Claude",
    icon: "claude",
    aliases: "anthropic claude code",
  },
  {
    id: "codex",
    website: "https://chatgpt.com",
    name: "ChatGPT / Codex",
    icon: "openai",
    aliases: "chatgpt codex openai",
  },
  {
    id: "chatgpt_api",
    website: "https://chatgpt.com",
    name: "ChatGPT API",
    icon: "openai",
    aliases: "openai sign in with chatgpt siwc",
  },
  {
    id: "github_copilot",
    website: "https://github.com",
    name: "GitHub Copilot",
    icon: "github",
    aliases: "github copilot",
  },
  {
    id: "gemini_cli",
    website: "https://geminicli.com",
    name: "Gemini CLI",
    icon: "geminicli",
    aliases: "google gemini cli code assist",
  },
  {
    id: "antigravity",
    website: "https://antigravity.google",
    name: "Antigravity",
    icon: "antigravity",
    aliases: "google antigravity",
  },
  {
    id: "xai_oauth",
    website: "https://grok.com",
    name: "Grok",
    icon: "xai",
    aliases: "xai grok supergrok",
  },
] as const;
export type SubscriptionProviderId =
  (typeof subscriptionProviders)[number]["id"];
