# Routing Reference Attribution

Vary AI adapts selected routing, catalog, gateway, and interface behavior from
[yetone/magpie](https://github.com/yetone/magpie), Copyright (c) 2026 yetone,
under the MIT License. The complete copyright and permission notice is retained
unchanged in [LICENSE](LICENSE). Renamed local assets and modules retain this
attribution.

## Pinned source revisions

- `3bce283df43c3ce733e457daa16a69ac38bbb5fd`: resource-oriented provider and
  Agent management, the routing workspace, and provider/import artwork under
  `internal/gui/assets/icons/`. Original vendor files are `groq.svg`,
  `together-color.svg`, `fireworks-color.svg`, `ai302-color.svg`, and `zai.svg`.
  Original import files are `app-cc-switch.png`, `claudecode-color.svg`,
  `codex-color.svg`, and `magpie.svg`.
- `c4551dfcacf67536180291da402ded1d1316a2d8`: protocol review and Gemini
  tool-result role/image adaptation. Reviewed sources under `internal/gateway/`
  are `ir.go`, `gateway.go`, `gemini.go`, `translate_severed_test.go`, and
  `gemini_function_parts_test.go`.
- `e5dd86965b56a8a176790e2c1ade09f9ef554208`: the original intent-routing
  proposal, referencing `internal/gateway/rules.go`, `classify.go`, `decide.go`,
  and `internal/provider/grouprule.go`.
- `508911e431f752052d1c8374f89de7ec75f291ac`: routing-row motion from
  `internal/gui/assets/app.js` and `plugins/routing/client.js`; pool strategies
  from `internal/routes/strategies.go`, `fastest.go`, `round.go`, and
  `internal/gateway/speed.go`.
- `0414bbeddbfc2028508b610fd1e7c194143454a0`: capacity and compaction
  publication from `internal/provider/models.go`, `group.go`, and
  `internal/codexcat/codexcat.go`.

Other adapted catalog helpers include `internal/catalog/family.go`,
`catalog.SameModel`, `internal/provider/grouptemplate.go`, and the usage-purpose
catalog. The behavioral limits remain documented in Vary's architecture and
design guides; an attribution does not establish complete compatibility.

## Compatibility literals

Internal selectors, variable names, assets, and design filenames use Vary or
descriptive names. A few external format identifiers remain literal so existing
files and integrations continue to work:

- Provider imports retain the source ID `magpie`, the optional IPC argument
  `magpiePath`, the default `~/.config/magpie` / `$XDG_CONFIG_HOME/magpie`
  locations, and recognition of `magpie-models.json` and the `magpie` provider
  identity when avoiding imports of a client's own generated gateway entries.
- Portable routing-group JSON retains `magpie: "routing-group"`, version 1,
  and accepts the `magpie/` model-reference prefix.
- Compatibility tests retain those exact synthetic identifiers. They must not
  be replaced with escaped or dynamically assembled spellings to conceal them.

The renderer labels the portable source **Provider backup**. Import still reads
the original source files, keeps credentials in the backend, and does not modify
the source application.
