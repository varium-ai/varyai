import { spawnSync } from "node:child_process";

export function command(
  program,
  args,
  { env = process.env, capture = false, allowFailure = false } = {},
) {
  // Pass credentials through the environment, never command arguments.
  const result = spawnSync(program, args, {
    env,
    encoding: "utf8",
    stdio: capture ? "pipe" : "inherit",
  });
  if ((result.error || result.status !== 0) && !allowFailure) {
    // Do not append arguments, child output or spawn error details.
    throw new Error(
      `${program} failed (${result.status ?? "unable to start"})`,
    );
  }
  return result;
}
