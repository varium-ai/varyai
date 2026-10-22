# Upstream provenance

Vary AI is an independent derivative of [CC Switch](https://github.com/farion1231/cc-switch). Its recorded common baseline is [`bd247a4adb92adfee0f065f0bdaac9580cfc276a`](https://github.com/farion1231/cc-switch/commit/bd247a4adb92adfee0f065f0bdaac9580cfc276a). That commit and its 2,438 ancestors are retained as their original Git objects.

The codebase combines inherited functionality, subsequent upstream adaptations, and Vary AI contributions. The baseline does not delimit all reused code, and a consolidated commit does not imply sole authorship of its contents. Contributor credit is recorded in [Contributors](../CONTRIBUTORS.md).

CC Switch is licensed under MIT at this baseline. The original [license](../LICENSE) and its copyright notice are preserved:

```text
Copyright (c) 2025 Jason Young
```

Vary AI is also distributed under MIT. Copies or substantial portions of the software must retain the copyright and permission notices. Dependencies and other third-party materials retain their own licenses and notices.

## Retained history boundary

| Reference                    | Recorded value                                                                                                                        |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Upstream repository          | [farion1231/cc-switch](https://github.com/farion1231/cc-switch)                                                                       |
| Retained upstream baseline   | [`bd247a4adb92adfee0f065f0bdaac9580cfc276a`](https://github.com/farion1231/cc-switch/commit/bd247a4adb92adfee0f065f0bdaac9580cfc276a) |
| Baseline commit subject      | `fix(proxy): omit missing tool descriptions instead of serializing null (#7319)`                                                      |
| Original upstream history    | 2,439 unchanged commits, including the baseline                                                                                       |
| Upstream license at baseline | [MIT](https://github.com/farion1231/cc-switch/blob/bd247a4adb92adfee0f065f0bdaac9580cfc276a/LICENSE)                                  |

Vary AI development was consolidated into a source snapshot whose parent is this baseline. Consolidation does not change the original upstream commits or make the snapshot author the sole author of the resulting code. The recorded boundary identifies the retained ancestry; later adaptations are documented separately below.

To inspect the baseline and its retained history in a full clone:

```bash
git show --no-patch --format=fuller bd247a4adb92adfee0f065f0bdaac9580cfc276a
git rev-list --count bd247a4adb92adfee0f065f0bdaac9580cfc276a
git merge-base --is-ancestor bd247a4adb92adfee0f065f0bdaac9580cfc276a HEAD
```

The count is `2439`; the ancestry check exits successfully. A shallow clone may omit the objects needed for these checks.

## Subsequent upstream contributions

After the original separation, VaryAI continued to incorporate CC Switch changes in provider handling, proxy behavior, configuration, database management, and the interface. The following examples were verified by matching complete per-file patches using `git patch-id --stable`.

The `LICENSE` file at each revision listed below matches the retained MIT license. The patch matches document these specific file adaptations; they do not establish that each entire commit was cherry-picked or enumerate every upstream import.

| Original upstream commit                                                                                                              | Files with matching patches                                         |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| [`b9c2739392434600523638ad026420660d674892`](https://github.com/farion1231/cc-switch/commit/b9c2739392434600523638ad026420660d674892) | `src-tauri/src/database/backup.rs`, `src-tauri/src/database/mod.rs` |
| [`993ce2f36cbac39feca5909f6fe1bf5b2db1acdd`](https://github.com/farion1231/cc-switch/commit/993ce2f36cbac39feca5909f6fe1bf5b2db1acdd) | `src-tauri/src/proxy/providers/codex_oauth_auth.rs`                 |
| [`bccfaf37f3cbe6f6202900b30afdedc721ef1055`](https://github.com/farion1231/cc-switch/commit/bccfaf37f3cbe6f6202900b30afdedc721ef1055) | `src/i18n/locales/zh.json`                                          |
| [`c715ee2b2d6f50f126b980290ea82400c396c2c3`](https://github.com/farion1231/cc-switch/commit/c715ee2b2d6f50f126b980290ea82400c396c2c3) | `src/components/providers/forms/CommonConfigEditor.tsx`             |
| [`4837fe2c9eed8655fc9dc84c135c26aa2fd34c00`](https://github.com/farion1231/cc-switch/commit/4837fe2c9eed8655fc9dc84c135c26aa2fd34c00) | `tests/integration/App.test.tsx`                                    |
| [`2c735bd9de73e2ca60099d24964bc674afa16d8e`](https://github.com/farion1231/cc-switch/commit/2c735bd9de73e2ca60099d24964bc674afa16d8e) | `tests/components/ProviderCard.usageExpansion.test.tsx`             |
| [`a659440b9fed4e86825a0bde788b7acb3da4c07b`](https://github.com/farion1231/cc-switch/commit/a659440b9fed4e86825a0bde788b7acb3da4c07b) | `src/components/prompts/PromptPanel.tsx`                            |

File paths identify the files when the patches were adapted. Subsequent Vary AI changes may have modified, renamed, or removed those files. These historical matches do not establish the current line-by-line extent of reused code.

Repository names, protocol identifiers, file paths, notices, and contributor attribution may retain upstream names where required for compatibility or accurate provenance. VaryAI-specific support and vulnerability reporting must use VaryAI-owned channels rather than upstream maintainers' accounts.
