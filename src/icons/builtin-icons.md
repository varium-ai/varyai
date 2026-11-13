# Built-in development and AI icons

The two provider icon pickers offer the 15 items selected from the
[AI Node icon collection](https://www.iconfont.cn/collections/detail?cid=53641)
by [haimianz](https://www.iconfont.cn/user/detail?uid=463840&nid=l6ZiceykXdJc).
The public collection was marked **Original / Free to use** on November 12, 2026. This is the source's usage label, not an assertion that the artwork is
covered by this repository's MIT license or that trademark rights are granted.

`builtin.ts` contains the public SVG paths with their original fills and view
boxes. It omits website-only attributes and sizes the SVGs to their containers.
The first three model symbols were explicitly selected together with the
generic artwork; Model Meta retains its source identity.

| Stored key            | Iconfont icon ID |
| --------------------- | ---------------- |
| generic-model-y       | 41911276         |
| generic-model-meta    | 41911277         |
| generic-model-cube    | 41911278         |
| generic-component     | 41912980         |
| generic-code          | 41914301         |
| generic-knowledge     | 41914386         |
| generic-api           | 41914388         |
| generic-model         | 41990249         |
| generic-output        | 46109093         |
| generic-tools         | 46109095         |
| generic-text          | 46109097         |
| generic-http          | 46109098         |
| generic-model-network | 46109099         |
| generic-intent        | 46109100         |
| generic-memory        | 46790575         |

Existing brand keys remain available to presets and saved configurations.
Only the offered picker palette changes. Both pickers use the shared search
function, which supports English names, Chinese concept keywords, and localized
labels in English, Simplified Chinese, Traditional Chinese, and Japanese.

New custom API providers receive a random icon from this palette when the
draft is created. The selected key is stored in the draft and saved with the
provider, so editing fields or reopening the editor does not change it.
Restoring the default picks and stores a fresh random built-in icon. The icon
field offers local image upload and the built-in palette; the acquisition
dropdown (automatic lookup, image URL, and letter icon) has been removed.
Existing saved icons and preset-specific defaults keep their current behavior.

## Additional candidates

These candidates are not part of the selected palette:

- The same collection's branch (41914387), loop (46109101), and conditional
  (46109102) icons match the selected artwork most closely.
- [Blue Artificial Intelligence](https://www.iconfont.cn/collections/detail?cid=52836),
  by sansanh, contains 80 icons including database, cloud computing, microchip,
  chatbot, and web programming. Its artwork has more detail and would need a
  visual review at 24 px. The source page identifies it as a public collection;
  redistribution terms have not been established.
