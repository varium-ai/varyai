import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const script = readFileSync("scripts/build-desktop.mjs", "utf8").replace(
  /^import .*;\n/gm,
  "",
);

function executeBuild(
  platform: string,
  argv: string[] = [],
  env: Record<string, string> = {},
  observed: { calls?: string[]; signatureAuthority?: string } = {},
) {
  const spawnSync = vi.fn((command: string, args: string[]) => {
    observed.calls?.push(command);
    return {
      status: 0,
      stdout: "",
      stderr:
        command === "codesign" && args.includes("-dv")
          ? `Authority=${observed.signatureAuthority ?? env.APPLE_SIGNING_IDENTITY}\n`
          : "",
    };
  });
  const logger = { log: vi.fn(), warn: vi.fn() };
  runInNewContext(script, {
    spawnSync,
    readFileSync,
    console: logger,
    process: {
      platform,
      argv: ["node", "scripts/build-desktop.mjs", ...argv],
      env,
      cwd: () => process.cwd(),
      exit: (status: number) => {
        throw new Error(`Unexpected exit: ${status}`);
      },
    },
  });
  return { calls: spawnSync.mock.calls, logger } as unknown as {
    calls: [
      string,
      string[],
      { shell?: boolean; env?: Record<string, string> },
    ][];
    logger: typeof logger;
  };
}

function build(platform: string, argv: string[] = []) {
  return executeBuild(platform, argv).calls;
}

describe("desktop build config arguments", () => {
  it.each(["x64", "arm64"])(
    "passes valid config through the Windows %s command shell",
    (arch) => {
      const [command, args, options] = build("win32", ["--arch", arch]).at(-1)!;
      expect(command).toBe("pnpm.cmd");
      expect(options.shell).toBe(true);
      expect(args.slice(0, 2)).toEqual(["tauri", "build"]);
      expect(args[args.indexOf("--bundles") + 1]).toBe("msi");
      if (arch === "arm64") {
        expect(args[args.indexOf("--target") + 1]).toBe(
          "aarch64-pc-windows-msvc",
        );
      } else {
        expect(args).not.toContain("--target");
      }
      // cmd.exe stripped these quotes in the failed GitHub Actions builds.
      const config = args[args.indexOf("--config") + 1].replace(/"/g, "");
      const contents = config.startsWith("{")
        ? config
        : readFileSync(config, "utf8");
      expect(JSON.parse(contents)).toEqual({
        bundle: { createUpdaterArtifacts: false },
      });
    },
  );

  it.each(["darwin", "linux"])(
    "disables updater artifacts on %s",
    (platform) => {
      const [, args] = build(platform).at(-1)!;
      const config = args[args.indexOf("--config") + 1];
      const contents = config.startsWith("{")
        ? config
        : readFileSync(config, "utf8");
      expect(JSON.parse(contents)).toEqual({
        bundle: { createUpdaterArtifacts: false },
      });
    },
  );

  it("keeps updater artifacts enabled when requested", () => {
    const [, args] = build("win32", ["--updater"]).at(-1)!;
    expect(args).not.toContain("--config");
  });

  it("builds an unsigned macOS development package without discovering keychain identities", () => {
    const { calls } = executeBuild("darwin");
    expect(calls.some(([command]) => command === "security")).toBe(false);
    expect(calls.some(([command]) => command === "codesign")).toBe(false);
    const [, args, options] = calls.find(([command]) => command === "pnpm")!;
    expect(args).toContain("--no-sign");
    expect(options.env).not.toHaveProperty("APPLE_SIGNING_IDENTITY");
  });

  it("uses an explicitly supplied signing identity and strips notarization credentials from the build", () => {
    const identity =
      "Developer ID Application: Example Maintainer (TESTTEAM01)";
    const { calls, logger } = executeBuild("darwin", [], {
      APPLE_SIGNING_IDENTITY: identity,
      APPLE_ID: "synthetic-account@example.invalid",
      APPLE_PASSWORD: "synthetic-notary-password",
      APPLE_API_KEY: "synthetic-api-key",
      APPLE_API_KEY_PATH: "synthetic-private-key-path",
      APPLE_API_ISSUER: "synthetic-issuer",
    });
    const [, args, options] = calls.find(([command]) => command === "pnpm")!;
    expect(args).not.toContain("--no-sign");
    expect(options.env?.APPLE_SIGNING_IDENTITY).toBe(identity);
    for (const key of [
      "APPLE_ID",
      "APPLE_PASSWORD",
      "APPLE_API_KEY",
      "APPLE_API_KEY_PATH",
      "APPLE_API_ISSUER",
    ]) {
      expect(options.env).not.toHaveProperty(key);
    }
    expect(
      calls.some(
        ([command, values]) =>
          command === "codesign" && values.includes(identity),
      ),
    ).toBe(true);
    expect(
      JSON.stringify([...logger.log.mock.calls, ...logger.warn.mock.calls]),
    ).not.toContain("synthetic-notary-password");
  });

  it.each([
    "APPLE_SIGNING_IDENTITY",
    "APPLE_TEAM_ID",
    "NOTARY_KEYCHAIN_PROFILE",
  ])(
    "rejects --notarize without %s before invoking external tools",
    (missing) => {
      const env: Record<string, string> = {
        APPLE_SIGNING_IDENTITY:
          "Developer ID Application: Example Maintainer (TESTTEAM01)",
        APPLE_TEAM_ID: "TESTTEAM01",
        NOTARY_KEYCHAIN_PROFILE: "example-release",
      };
      delete env[missing];
      const calls: string[] = [];
      expect(() =>
        executeBuild("darwin", ["--notarize"], env, { calls }),
      ).toThrow(missing);
      expect(calls).toEqual([]);
    },
  );

  it.each([
    ["APPLE_TEAM_ID", "invalid-team"],
    ["NOTARY_KEYCHAIN_PROFILE", "profile\nsynthetic-password"],
  ])("rejects invalid %s before a notarized build", (key, value) => {
    const calls: string[] = [];
    expect(() =>
      executeBuild(
        "darwin",
        ["--notarize"],
        {
          APPLE_SIGNING_IDENTITY:
            "Developer ID Application: Example Maintainer (TESTTEAM01)",
          APPLE_TEAM_ID: "TESTTEAM01",
          NOTARY_KEYCHAIN_PROFILE: "example-release",
          [key]: value,
        },
        { calls },
      ),
    ).toThrow(key);
    expect(calls).toEqual([]);
  });

  it.each([
    ["APPLE_SIGNING_IDENTITY", "leading"],
    ["APPLE_SIGNING_IDENTITY", "trailing"],
    ["APPLE_TEAM_ID", "leading"],
    ["APPLE_TEAM_ID", "trailing"],
    ["NOTARY_KEYCHAIN_PROFILE", "leading"],
    ["NOTARY_KEYCHAIN_PROFILE", "trailing"],
  ])("rejects %s with %s whitespace before any tool runs", (key, side) => {
    const env: Record<string, string> = {
      APPLE_SIGNING_IDENTITY:
        "Developer ID Application: Example Maintainer (TESTTEAM01)",
      APPLE_TEAM_ID: "TESTTEAM01",
      NOTARY_KEYCHAIN_PROFILE: "example-release",
    };
    env[key] = side === "leading" ? ` ${env[key]}` : `${env[key]} `;
    const calls: string[] = [];
    expect(() =>
      executeBuild("darwin", ["--notarize"], env, { calls }),
    ).toThrow(key);
    expect(calls).toEqual([]);
  });

  it.each(["leading", "trailing"])(
    "rejects %s signing-identity whitespace even without notarization",
    (side) => {
      const identity =
        "Developer ID Application: Example Maintainer (TESTTEAM01)";
      const calls: string[] = [];
      expect(() =>
        executeBuild(
          "darwin",
          [],
          {
            APPLE_SIGNING_IDENTITY:
              side === "leading" ? ` ${identity}` : `${identity} `,
          },
          { calls },
        ),
      ).toThrow("APPLE_SIGNING_IDENTITY");
      expect(calls).toEqual([]);
    },
  );

  it("rejects an Authority that only starts with the selected identity", () => {
    const calls: string[] = [];
    expect(() =>
      executeBuild(
        "darwin",
        [],
        { APPLE_SIGNING_IDENTITY: "Developer ID Application: Example" },
        {
          calls,
          signatureAuthority:
            "Developer ID Application: Example Maintainer (TESTTEAM01)",
        },
      ),
    ).toThrow("App was not signed with the selected Developer ID identity");
    expect(calls).not.toContain("hdiutil");
  });

  it("notarizes through the configured profile script after a signed build", () => {
    const { calls } = executeBuild("darwin", ["--notarize"], {
      APPLE_SIGNING_IDENTITY:
        "Developer ID Application: Example Maintainer (TESTTEAM01)",
      APPLE_TEAM_ID: "TESTTEAM01",
      NOTARY_KEYCHAIN_PROFILE: "example-release",
      APPLE_PASSWORD: "synthetic-password",
    });
    const pnpmIndex = calls.findIndex(([command]) => command === "pnpm");
    expect(calls[pnpmIndex][2].env).not.toHaveProperty("APPLE_PASSWORD");
    const notaryIndex = calls.findIndex(
      ([command, args]) =>
        command === "bash" && args[0] === "scripts/notarize-macos.sh",
    );
    expect(notaryIndex).toBeGreaterThan(pnpmIndex);
  });
});
