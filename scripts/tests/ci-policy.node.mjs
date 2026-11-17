import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { isVerifiedMergePush } from "../ci-policy.mjs";

const env = {
  GITHUB_EVENT_NAME: "push",
  GITHUB_REPOSITORY: "synthetic/repo",
  GITHUB_REF_NAME: "main",
  GITHUB_SHA: "synthetic-merge",
  GITHUB_API_URL: "https://api.github.com",
  GH_TOKEN: "synthetic-token",
};
const merged = {
  merged_at: "2026-11-17T00:00:00Z",
  merge_commit_sha: env.GITHUB_SHA,
  head: { sha: "synthetic-head" },
  base: { ref: "main", repo: { full_name: env.GITHUB_REPOSITORY } },
};
function responses(pulls, runs = [{ conclusion: "success" }]) {
  return async (url) => ({
    ok: true,
    json: async () =>
      url.includes("/commits/") ? pulls : { workflow_runs: runs },
  });
}

test("a merged PR with successful CI skips its duplicate main run", async () => {
  assert.equal(await isVerifiedMergePush(env, responses([merged])), true);
});

test("PR and manual runs always retain their checks", async () => {
  for (const event of ["pull_request", "workflow_dispatch"]) {
    assert.equal(
      await isVerifiedMergePush({ ...env, GITHUB_EVENT_NAME: event }, () => {
        assert.fail("Only pushes need an API lookup");
      }),
      false,
    );
  }
});

test("direct pushes and unrelated or unmerged PRs retain checks", async () => {
  for (const pulls of [
    [],
    [{ ...merged, merged_at: null }],
    [{ ...merged, merge_commit_sha: "synthetic-other" }],
    [{ ...merged, base: { ...merged.base, ref: "other" } }],
    [
      {
        ...merged,
        base: { ...merged.base, repo: { full_name: "other/repo" } },
      },
    ],
  ]) {
    assert.equal(await isVerifiedMergePush(env, responses(pulls)), false);
  }
});

test("missing, pending, failed, cancelled or skipped PR CI retains checks", async () => {
  for (const conclusion of [null, "failure", "cancelled", "skipped"]) {
    assert.equal(
      await isVerifiedMergePush(env, responses([merged], [{ conclusion }])),
      false,
    );
  }
  assert.equal(await isVerifiedMergePush(env, responses([merged], [])), false);
});

test("API errors and malformed responses retain checks", async () => {
  for (const request of [
    async () => {
      throw new Error("synthetic unavailable");
    },
    async () => ({ ok: false }),
    async () => ({ ok: true, json: async () => null }),
  ]) {
    assert.equal(await isVerifiedMergePush(env, request), false);
  }
});

test("the CLI writes the workflow output for a non-push event", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "varyai-ci-policy-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const output = path.join(dir, "output");
  const run = spawnSync(process.execPath, ["scripts/ci-policy.mjs"], {
    cwd: new URL("../../", import.meta.url),
    env: { ...env, GITHUB_EVENT_NAME: "pull_request", GITHUB_OUTPUT: output },
    encoding: "utf8",
  });
  assert.equal(run.status, 0, run.stderr);
  assert.equal(readFileSync(output, "utf8"), "duplicate_merge=false\n");
});
