import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export async function isVerifiedMergePush(env, request = fetch) {
  if (env.GITHUB_EVENT_NAME !== "push") return false;
  const repository = env.GITHUB_REPOSITORY;
  const api = `${env.GITHUB_API_URL}/repos/${repository}`;
  const options = {
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${env.GH_TOKEN}`,
      "X-GitHub-Api-Version": "2022-11-28",
    },
    signal: AbortSignal.timeout(15000),
  };
  try {
    const response = await request(
      `${api}/commits/${env.GITHUB_SHA}/pulls`,
      options,
    );
    if (!response.ok) return false;
    const pulls = await response.json();
    const merged = pulls.find(
      (pr) =>
        pr.merged_at &&
        pr.merge_commit_sha === env.GITHUB_SHA &&
        pr.base.ref === env.GITHUB_REF_NAME &&
        pr.base.repo.full_name === repository,
    );
    if (!merged) return false;
    const runs = await request(
      `${api}/actions/workflows/ci.yml/runs?event=pull_request&head_sha=${merged.head.sha}&per_page=1`,
      options,
    );
    if (!runs.ok) return false;
    const result = await runs.json();
    return result.workflow_runs[0]?.conclusion === "success";
  } catch {
    // An unavailable API must keep the checks enabled.
    return false;
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const duplicate = await isVerifiedMergePush(process.env);
  appendFileSync(process.env.GITHUB_OUTPUT, `duplicate_merge=${duplicate}\n`);
}
