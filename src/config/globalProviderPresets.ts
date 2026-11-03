export interface GlobalProviderPreset {
  id: string;
  name: string;
  baseUrl: string;
  apiFormat: "anthropic" | "openai_chat" | "openai_responses" | "gemini_native";
  kind: "vendor" | "relay";
  icon: string;
  authType?: "api-key" | "bearer";
  endpoints?: Partial<Record<GlobalProviderPreset["apiFormat"], string>>;
  regionLabel?: "region" | "plan";
  regions?: Array<{
    id: string;
    name: string;
    endpoints: NonNullable<GlobalProviderPreset["endpoints"]>;
  }>;
  china?: {
    name: string;
    endpoints: NonNullable<GlobalProviderPreset["endpoints"]>;
  };
}

// Global API presets are independent of any client application's configuration.
export const globalProviderPresets: GlobalProviderPreset[] = [
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    apiFormat: "openai_responses",
    kind: "vendor",
    icon: "openai",
    endpoints: {
      openai_responses: "https://api.openai.com/v1",
      openai_chat: "https://api.openai.com/v1",
    },
  },
  {
    id: "anthropic",
    name: "Anthropic",
    baseUrl: "https://api.anthropic.com",
    apiFormat: "anthropic",
    kind: "vendor",
    icon: "anthropic",
    authType: "api-key",
    endpoints: { anthropic: "https://api.anthropic.com" },
  },
  {
    id: "google",
    name: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    apiFormat: "gemini_native",
    kind: "vendor",
    icon: "gemini",
    endpoints: {
      gemini_native: "https://generativelanguage.googleapis.com/v1beta",
      openai_chat: "https://generativelanguage.googleapis.com/v1beta/openai",
    },
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com/v1",
    apiFormat: "openai_chat",
    kind: "vendor",
    icon: "deepseek",
    endpoints: {
      openai_chat: "https://api.deepseek.com/v1",
      openai_responses: "https://api.deepseek.com/v1",
      anthropic: "https://api.deepseek.com/anthropic",
    },
  },
  {
    id: "moonshot",
    name: "Kimi",
    baseUrl: "https://api.moonshot.ai/v1",
    apiFormat: "openai_chat",
    kind: "vendor",
    icon: "kimi",
    endpoints: {
      openai_chat: "https://api.moonshot.ai/v1",
      anthropic: "https://api.moonshot.ai/anthropic",
    },
    china: {
      name: "Kimi (China)",
      endpoints: {
        openai_chat: "https://api.moonshot.cn/v1",
        anthropic: "https://api.moonshot.cn/anthropic",
      },
    },
  },
  {
    id: "zai",
    name: "Z.ai",
    baseUrl: "https://api.z.ai/api/paas/v4",
    apiFormat: "openai_chat",
    kind: "vendor",
    icon: "zai",
    endpoints: {
      openai_chat: "https://api.z.ai/api/paas/v4",
      anthropic: "https://api.z.ai/api/anthropic",
    },
    regionLabel: "plan",
    regions: [
      {
        id: "api",
        name: "Pay as you go",
        endpoints: {
          openai_chat: "https://api.z.ai/api/paas/v4",
          anthropic: "https://api.z.ai/api/anthropic",
        },
      },
      {
        id: "coding",
        name: "Coding Plan",
        endpoints: {
          openai_chat: "https://api.z.ai/api/coding/paas/v4",
          openai_responses: "https://api.z.ai/api/v1",
          anthropic: "https://api.z.ai/api/anthropic",
        },
      },
    ],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    apiFormat: "openai_chat",
    kind: "relay",
    icon: "openrouter",
    endpoints: {
      openai_chat: "https://openrouter.ai/api/v1",
      anthropic: "https://openrouter.ai/api",
    },
  },
  {
    id: "xai",
    name: "xAI",
    kind: "vendor",
    icon: "xai",
    baseUrl: "https://api.x.ai/v1",
    apiFormat: "openai_responses",
    endpoints: {
      openai_chat: "https://api.x.ai/v1",
      openai_responses: "https://api.x.ai/v1",
      anthropic: "https://api.x.ai",
    },
  },
  {
    id: "kimi-code",
    name: "Kimi Code",
    kind: "vendor",
    icon: "kimi-code",
    baseUrl: "https://api.kimi.ai/coding/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://api.kimi.ai/coding/v1",
      anthropic: "https://api.kimi.ai/coding",
    },
    china: {
      name: "Kimi Code (China)",
      endpoints: {
        openai_chat: "https://api.kimi.com/coding/v1",
        anthropic: "https://api.kimi.com/coding",
      },
    },
  },
  {
    id: "zhipu",
    name: "Zhipu GLM",
    kind: "vendor",
    icon: "zhipu",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://open.bigmodel.cn/api/paas/v4",
      anthropic: "https://open.bigmodel.cn/api/anthropic",
    },
    regionLabel: "plan",
    regions: [
      {
        id: "api",
        name: "Pay as you go",
        endpoints: {
          openai_chat: "https://open.bigmodel.cn/api/paas/v4",
          anthropic: "https://open.bigmodel.cn/api/anthropic",
        },
      },
      {
        id: "coding",
        name: "Coding Plan",
        endpoints: {
          openai_chat: "https://open.bigmodel.cn/api/coding/paas/v4",
          openai_responses: "https://open.bigmodel.cn/api/v1",
          anthropic: "https://open.bigmodel.cn/api/anthropic",
        },
      },
    ],
  },
  {
    id: "minimax",
    name: "MiniMax",
    kind: "vendor",
    icon: "minimax",
    baseUrl: "https://api.minimax.io/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://api.minimax.io/v1",
      anthropic: "https://api.minimax.io/anthropic",
    },
    china: {
      name: "MiniMax (China)",
      endpoints: {
        openai_chat: "https://api.minimaxi.com/v1",
        anthropic: "https://api.minimaxi.com/anthropic",
      },
    },
  },
  {
    id: "qwen",
    name: "Qwen",
    kind: "vendor",
    icon: "qwen",
    baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      anthropic: "https://dashscope-intl.aliyuncs.com/apps/anthropic",
    },
    china: {
      name: "Qwen (China)",
      endpoints: {
        openai_chat: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        anthropic: "https://dashscope.aliyuncs.com/apps/anthropic",
      },
    },
  },
  {
    id: "mistral",
    name: "Mistral",
    kind: "vendor",
    icon: "mistral",
    baseUrl: "https://api.mistral.ai/v1",
    apiFormat: "openai_chat",
    endpoints: { openai_chat: "https://api.mistral.ai/v1" },
  },
  {
    id: "groq",
    name: "Groq",
    kind: "vendor",
    icon: "groq",
    baseUrl: "https://api.groq.com/openai/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://api.groq.com/openai/v1",
      openai_responses: "https://api.groq.com/openai/v1",
    },
  },
  {
    id: "together",
    name: "Together AI",
    kind: "relay",
    icon: "together",
    baseUrl: "https://api.together.xyz/v1",
    apiFormat: "openai_chat",
    endpoints: { openai_chat: "https://api.together.xyz/v1" },
  },
  {
    id: "fireworks",
    name: "Fireworks",
    kind: "relay",
    icon: "fireworks",
    baseUrl: "https://api.fireworks.ai/inference/v1",
    apiFormat: "openai_chat",
    endpoints: { openai_chat: "https://api.fireworks.ai/inference/v1" },
  },
  {
    id: "siliconflow",
    name: "SiliconFlow",
    kind: "relay",
    icon: "siliconflow",
    baseUrl: "https://api.siliconflow.com/v1",
    apiFormat: "openai_chat",
    endpoints: { openai_chat: "https://api.siliconflow.com/v1" },
    china: {
      name: "SiliconFlow (China)",
      endpoints: { openai_chat: "https://api.siliconflow.cn/v1" },
    },
  },
  {
    id: "nvidia",
    name: "NVIDIA NIM",
    kind: "relay",
    icon: "nvidia",
    baseUrl: "https://integrate.api.nvidia.com/v1",
    apiFormat: "openai_chat",
    endpoints: { openai_chat: "https://integrate.api.nvidia.com/v1" },
  },
  {
    id: "modelscope",
    name: "ModelScope",
    kind: "relay",
    icon: "modelscope",
    baseUrl: "https://api-inference.modelscope.cn/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://api-inference.modelscope.cn/v1",
      openai_responses: "https://api-inference.modelscope.cn/v1",
    },
  },
  {
    id: "aihubmix",
    name: "AiHubMix",
    kind: "relay",
    icon: "aihubmix",
    baseUrl: "https://aihubmix.com/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://aihubmix.com/v1",
      anthropic: "https://aihubmix.com",
    },
  },
  {
    id: "302ai",
    name: "302.AI",
    kind: "relay",
    icon: "ai302",
    baseUrl: "https://api.302.ai/v1",
    apiFormat: "openai_chat",
    endpoints: {
      openai_chat: "https://api.302.ai/v1",
      anthropic: "https://api.302.ai",
    },
  },
];
