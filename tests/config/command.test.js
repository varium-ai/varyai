import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { command } from "../../scripts/lib/command.mjs";

describe("public command runner", () => {
  it("passes the environment and captures output when requested", () => {
    const result = command(
      process.execPath,
      ["-e", "process.stdout.write(process.env.COMMAND_TEST_MESSAGE)"],
      {
        capture: true,
        env: { ...process.env, COMMAND_TEST_MESSAGE: "fixture output" },
      },
    );
    expect(result.status).toBe(0);
    expect(result.stdout).toBe("fixture output");
  });

  it("inherits standard streams by default", () => {
    const result = command(process.execPath, ["-e", "process.exit(0)"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toBeNull();
    expect(result.stderr).toBeNull();
  });

  it("reports a failure without copying arguments, output or environment values", () => {
    let failure;
    try {
      command(
        process.execPath,
        [
          "-e",
          "process.stderr.write(process.env.COMMAND_TEST_SECRET); process.exit(9)",
        ],
        {
          capture: true,
          env: { ...process.env, COMMAND_TEST_SECRET: "synthetic-test-secret" },
        },
      );
    } catch (error) {
      failure = error;
    }
    expect(failure?.message).toBe(`${process.execPath} failed (9)`);
  });

  it("returns an allowed failure so callers can inspect the result", () => {
    const result = command(
      process.execPath,
      ["-e", "process.stderr.write('fixture failure'); process.exit(7)"],
      { capture: true, allowFailure: true },
    );
    expect(result.status).toBe(7);
    expect(result.stderr).toBe("fixture failure");
  });

  it("handles a missing executable without exposing the spawn error details", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "varyai-command-"));
    try {
      const program = path.join(directory, "missing-tool");
      expect(() => command(program, [], { capture: true })).toThrow(
        `${program} failed (unable to start)`,
      );
      const result = command(program, [], {
        capture: true,
        allowFailure: true,
      });
      expect(result.status).toBeNull();
      expect(result.error.code).toBe("ENOENT");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
