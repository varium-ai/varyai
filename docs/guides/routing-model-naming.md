# Model naming in the Most capable first template

Sources checked on 2026-11-19. These are Vary AI's recommendation rules for
new **Most capable first** drafts, not vendor endorsements of a global
leaderboard. The rules only inspect the user's configured routing catalog.
They do not fetch vendor pages at runtime, certify inference access, rewrite
model IDs, or change saved groups or the other routing templates.

## Vendor rules

| Family                  | Recommendation rule                                                                                                                                                                                                                                  | Official reference                                                                                                                                                                                                                                      |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OpenAI GPT              | Astra and GPT Pro are high-tier lines; GPT-5.6 Sol is a documented high-tier exception. In the GPT-6 standard tier, Sol precedes Terra. Luna, Mini, and Nano are efficiency lines.                                                                   | [Model catalog](https://developers.openai.com/api/docs/models.md), [model selection](https://developers.openai.com/api/docs/guides/model-selection.md)                                                                                                  |
| Anthropic Claude        | Fable precedes Opus in the high tier, followed by Sonnet and then Haiku. Legacy orderings of the edition and version, Bedrock namespaces, and dated snapshots are recognized without rewriting the original ID.                                      | [Choosing a model](https://platform.claude.com/docs/en/about-claude/models/choosing-a-model.md), [IDs and versioning](https://platform.claude.com/docs/en/about-claude/models/model-ids-and-versions)                                                   |
| Google Gemini           | Argon and Carbon share the high tier with the Pro line; Neon is standard. Hydrogen and Flash are efficiency lines; Flash-Lite ranks below regular Flash within that tier. No precedence is inferred between Argon and Carbon from their names alone. | [Gemini 4 family](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/start/get-started-with-gemini-4?hl=en), [model catalog](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/google-models)                    |
| DeepSeek                | V4 Pro is high-tier; the Flash line, including the unversioned `deepseek-flash` and the legacy V4 Flash spelling, is efficiency-tier. Unversioned IDs do not acquire a guessed version or replace a saved ID.                                        | [Models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)                                                                                                                                                                                |
| Alibaba Qwen            | Max is high-tier, Plus is standard, and Flash and Turbo are efficiency variants. Both `qwen4-max` and unversioned spellings such as `qwen-max` are recognized.                                                                                       | [Model Studio catalog](https://www.alibabacloud.com/help/en/model-studio/models), [Qwen4-Max](https://www.alibabacloud.com/help/en/model-studio/qwen4-max)                                                                                              |
| Z.ai GLM                | The base GLM-5 line is high-tier. Turbo is handled separately, not treated as a synonym for a smaller model. The Flash/FlashX and Air/AirX lines are efficiency-tier.                                                                                | [Model overview](https://docs.bigmodel.cn/cn/guide/start/model-overview.md)                                                                                                                                                                             |
| Moonshot Kimi           | K3 and K3.1 Pro are high-tier. K3.1 Pro Highspeed keeps the same naming rank as Pro; speed alone does not imply lower capability.                                                                                                                    | [Model list](https://platform.kimi.ai/docs/models.md)                                                                                                                                                                                                   |
| xAI Grok                | Grok 4.5 to 4.7 are the high tier. The 4.7 Fast variant stays with 4.7, and the documented 4.20 series sorts before 4.3, despite its name.                                                                                                           | [Grok 4.7](https://docs.x.ai/developers/grok-4-7.md), [release notes](https://docs.x.ai/developers/release-notes.md)                                                                                                                                    |
| MiniMax                 | M3 and M3.1 Flash are both documented as frontier models. No preference is inferred between the two tracks. Highspeed is not a capability downgrade.                                                                                                 | [Language models](https://platform.minimax.io/docs/guides/models-intro.md), [model list](https://platform.minimax.io/docs/faq/models.md)                                                                                                                |
| Mistral                 | Large, Medium, Small and Ministral use their product-line positioning as a recommendation hint. Numbered model generations and `YYMM` IDs are compared in separate tracks, not as if they were the same numbering system.                            | [Model catalog](https://docs.mistral.ai/models), [Large](https://docs.mistral.ai/models/mistral-large-4-0.md), [Medium](https://docs.mistral.ai/models/mistral-medium-3-5-26-04.md), [Small](https://docs.mistral.ai/models/mistral-small-4-0-26-03.md) |
| ByteDance Seed / Doubao | Pro is high-tier, Lite and Turbo are standard, and Mini is efficiency-tier. The `seed`, `doubao-seed`, and `dola-seed` spellings use the same family rule.                                                                                           | [Seed2.0](https://seed.bytedance.com/en/seed2), [Seed2.1](https://seed.bytedance.com/en/seed2_1)                                                                                                                                                        |
| Xiaomi MiMo             | Pro is high-tier, the unqualified version is standard, and the Flash line is efficiency-tier. A Pro serving variant such as UltraSpeed keeps the same naming rank as Pro.                                                                            | [Xiaomi MiMo](https://mimo.xiaomi.com/), [Pro model card](https://huggingface.co/XiaomiMiMo/MiMo-V2.5-Pro), [Flash model card](https://github.com/XiaomiMiMo/MiMo-V2-Flash)                                                                             |

## Ranking and updates

1. Start with eligible catalog entries. A missing model is never injected into a
   draft. An unfamiliar name or edition remains neutral; arbitrary words such as
   `pro` or `fast` and parameter counts do not assign a capability tier on their own.
2. Rank by tier first. Within a tier, original-maker metadata, the existing
   output-price band, and release date determine positions across families. Within
   a recognized family and tier, use its edition preferences and then compare
   version components numerically. Unresolved ties retain input order. This is not
   a cross-vendor quality score.
3. The draft starts with at most three suggested members, preferring distinct
   suppliers before reusing one. **Add model** appends more members to the same
   ordered list. **Apply recommended order** ranks only the current members.
   Review the result before saving.

Use **Sync models** when a supplier adds a new model. A new version under a
recognized convention needs no model-by-model registration. Add a rule only when
an official source documents a new position, naming convention, or version
exception. Include the source and a regression case. No documentation scrape or
external code download is part of opening the template.

## QA checks

- Use a single synthetic supplier to isolate ranking from supplier diversity.
  Put the input in reverse order and check each family's expected order using the
  cases in [routing template tests](../../tests/utils/routingTemplates.test.ts).
- In particular, check that Bedrock and dated Claude IDs are compared without being
  renamed, Grok 4.7 Fast precedes 4.20, and Kimi Highspeed does not lose its tier.
- Use unfamiliar IDs with identical metadata to verify that suffixes alone do not
  reorder them. Then add a new version in a known edition and check the order in a
  fresh draft; the previously saved group must remain unchanged.
