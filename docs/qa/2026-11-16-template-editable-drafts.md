# Template entries open editable drafts

The user clarified that missing models should not prevent opening any template. This supersedes the earlier decision to disable Add when template requirements are missing.

All three template cards now allow opening the editor when models, cheaper alternatives or classifier support are missing, and while the model catalog is loading or failed. Missing-condition messages remain visible as guidance. Existing template suggestions are used when available; incomplete drafts let the user select models before saving. Empty drafts cannot be saved. Opening the editor does not save, publish or send a model request. Busy save/delete operations retain their existing concurrency guards.

## Verification

- Base revision: `6ea3162401a1f02dfcba080fa100c4fd7f5c150b`; macOS arm64, Node 20.20.2, pnpm 10.12.3.
- Five new expectations failed against the previous disabled behavior before the implementation change.
- Focused routing UI suite: 114 tests passed across two files, including all three templates without models, a loading/failed catalog, opening the editor and refusing to save an empty draft.
- Typecheck, renderer build and source formatting: passed. Logs: `/tmp/varyai-template-draft-*.log`.
- Full frontend tests: 4,247 passed across 318 files. Both affected test files also passed formatting checks.
- Shared macOS/Windows renderer change only; no backend/protocol/model-conversion changes or installed-client acceptance. This follow-up is not part of the already pushed single-model commit.
