# Provider import shortcut verification

Date: 2026-11-10 (Asia/Shanghai).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with unrelated working-tree changes preserved.
Host: macOS, Darwin 24.3.0, arm64; Node v24.11.0 and pnpm 10.12.3.

The provider list header now shows Import Provider immediately before Add Provider.
It opens the existing import dialog directly. The old import action and callback
were removed from provider selection. The existing translated label is reused in
English, Simplified Chinese, Japanese and Traditional Chinese. Header actions wrap
on narrow screens. Import completion refreshes the list without switching pages;
dialog focus returns to the import trigger.

Existing tests were updated first; three selected tests failed before the move.
After implementation, all 75 tests in ProviderAddSheet, ProviderParity,
ProviderImportDialog and RoutingUpstreamsPanel passed, including header order,
direct import, removal of the old entry and focus restoration.

| Check                 | Result  |
| --------------------- | ------- |
| `pnpm typecheck`      | Passed  |
| `pnpm build:renderer` | Passed  |
| `pnpm format:check`   | Passed  |
| `pnpm test:unit`      | Passed: 3691 tests across 287 files |
| `git diff --check`    | Passed  |

Tested working-tree file SHA-256 values:

```text
728d121a6eb5200db8d0f94b2cd57a8e66165dbffc35094ec1268fdea9b3a6ca  src/components/providers/ProviderAddSheet.tsx
d26648917ee7079cece3bbf9370cd5db2472c0d9692f239469f60ce62077bf44  src/components/providers/RoutingUpstreamsPanel.tsx
8bf79cd5fb574eaf8465dad6a4db7cf2396ec910ae70e6e31536b62c0110605a  src/components/providers/providers.css
c14fcb9c615b5ea80f42925edd9fe92c683bc28fc94d3cf75fa88080f54f8aa1  tests/components/ProviderAddSheet.test.tsx
b736f9f097a2d68600da88a38192992c82797da6bc624b90df1daef358ef6db7  tests/components/ProviderParity.test.tsx
```

Logs: `/tmp/vary-import-placement-*.log`. This follow-up changes renderer files
only. Backend, helper-script and production dependency checks were completed in
the preceding [batch deletion verification](2026-11-10-provider-batch-deletion.md)
and were not repeated for this UI move. No application was launched or installed,
and no commit, push or remote CI verification was performed. Results establish
local renderer verification, not cross-platform release acceptance.
