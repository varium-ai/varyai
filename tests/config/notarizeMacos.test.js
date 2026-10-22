import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const temporary = [];
const identity = "Developer ID Application: Example Maintainer (TESTTEAM01)";
const configuration = {
  APPLE_SIGNING_IDENTITY: identity,
  APPLE_TEAM_ID: "TESTTEAM01",
  NOTARY_KEYCHAIN_PROFILE: "example-release",
};

afterEach(() => {
  for (const directory of temporary.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), "varyai-notary-test-"));
  temporary.push(root);
  const bin = path.join(root, "bin");
  mkdirSync(bin);
  mkdirSync(path.join(root, "scripts"));
  for (const name of ["notarize-macos.sh", "build-and-notarize-macos.sh"]) {
    copyFileSync(path.join("scripts", name), path.join(root, "scripts", name));
  }
  writeFileSync(
    path.join(root, "package.json"),
    JSON.stringify({ version: "0.0.0" }),
  );
  mkdirSync(
    path.join(
      root,
      "src-tauri/target/universal-apple-darwin/release/bundle/macos/Vary AI.app",
    ),
    { recursive: true },
  );
  symlinkSync(process.execPath, path.join(bin, "node"));
  const mock = `#!/bin/bash
set -eu
name="$(basename "$0")"
printf '%s' "$name" >> "$MOCK_CALLS"
printf ' %s' "$@" >> "$MOCK_CALLS"
printf '\\n' >> "$MOCK_CALLS"
case "$name" in
  uname) echo Darwin ;;
  codesign)
    if [[ "$*" == *-dv* ]]; then
      printf 'Authority=%s\\nTeamIdentifier=%s\\n' "$APPLE_SIGNING_IDENTITY" "\${MOCK_CERT_TEAM:-$APPLE_TEAM_ID}" >&2
    fi ;;
  security) printf '1) fixture "%s"\\n' "$APPLE_SIGNING_IDENTITY" ;;
  xcrun)
    if [ "$1" = notarytool ]; then
      if [ "\${MOCK_FAIL_NOTARY:-}" = "$2" ]; then
        echo 'synthetic-secret-diagnostic'
        echo 'synthetic-secret-diagnostic' >&2
        exit 1
      fi
      if [ "$2" = submit ]; then
        printf '{"status":"Accepted","id":"synthetic-submission"}\\n'
      fi
    fi ;;
  ditto)
    if [[ "$*" == *-c* ]]; then touch "\${@: -1}"; else mkdir -p "\${@: -1}"; fi ;;
  hdiutil) if [ "$1" = create ]; then touch "\${@: -1}"; fi ;;
  shasum) echo 'synthetic-checksum fixture' ;;
esac
`;
  for (const name of [
    "uname",
    "codesign",
    "security",
    "xcrun",
    "ditto",
    "hdiutil",
    "spctl",
    "shasum",
    "pnpm",
  ]) {
    writeFileSync(path.join(bin, name), mock, { mode: 0o755 });
  }
  const callsPath = path.join(root, "calls.txt");
  writeFileSync(callsPath, "");
  return {
    run(script = "notarize-macos.sh", env = configuration, args = []) {
      const result = spawnSync(
        "/bin/bash",
        [path.join(root, "scripts", script), ...args],
        {
          cwd: root,
          encoding: "utf8",
          timeout: 10000,
          env: {
            PATH: `${bin}:/usr/bin:/bin`,
            HOME: root,
            MOCK_CALLS: callsPath,
            ...env,
          },
        },
      );
      return {
        ...result,
        calls: readFileSync(callsPath, "utf8"),
        output: result.stdout + result.stderr,
      };
    },
  };
}

describe.skipIf(process.platform === "win32")(
  "generic macOS notarization",
  () => {
    it.each([
      "APPLE_SIGNING_IDENTITY",
      "APPLE_TEAM_ID",
      "NOTARY_KEYCHAIN_PROFILE",
    ])(
      "fails without %s before signing, Keychain, or network operations",
      (missing) => {
        const env = { ...configuration };
        delete env[missing];
        const result = fixture().run("notarize-macos.sh", env);
        expect(result.status).not.toBe(0);
        expect(result.output).toContain(missing);
        expect(result.calls).not.toMatch(/(?:codesign|security|xcrun|pnpm) /);
      },
    );

    it("validates explicit configuration without calling system tools", () => {
      const result = fixture().run("notarize-macos.sh", configuration, [
        "--check-config",
      ]);
      expect(result.status, result.output).toBe(0);
      expect(result.calls).toBe("");
    });

    for (const script of ["notarize-macos.sh", "build-and-notarize-macos.sh"]) {
      it.each([
        ["APPLE_SIGNING_IDENTITY", "leading"],
        ["APPLE_SIGNING_IDENTITY", "trailing"],
        ["APPLE_TEAM_ID", "leading"],
        ["APPLE_TEAM_ID", "trailing"],
        ["NOTARY_KEYCHAIN_PROFILE", "leading"],
        ["NOTARY_KEYCHAIN_PROFILE", "trailing"],
      ])(
        `${script} rejects %s with %s whitespace before external operations`,
        (key, side) => {
          const env = { ...configuration };
          env[key] = side === "leading" ? ` ${env[key]}` : `${env[key]} `;
          const result = fixture().run(script, env);
          expect(result.status).not.toBe(0);
          expect(result.output).toContain(key);
          expect(result.calls).not.toMatch(/(?:pnpm|codesign|security|xcrun) /);
        },
      );
    }

    it("accepts the caller's identity and profile without provisioning Apple credentials", () => {
      const result = fixture().run();
      expect(result.status, result.output).toBe(0);
      expect(result.calls.match(/xcrun notarytool submit /g)).toHaveLength(2);
      expect(result.calls).toContain("--keychain-profile example-release");
      expect(result.calls).not.toMatch(
        /store-credentials|--apple-id|--password/,
      );
    });

    it("rejects a signed app from a different team before contacting Apple", () => {
      const result = fixture().run("notarize-macos.sh", {
        ...configuration,
        MOCK_CERT_TEAM: "OTHERTEAM0",
      });
      expect(result.status).not.toBe(0);
      expect(result.output).toContain("match");
      expect(result.calls).not.toMatch(/(?:security|xcrun) /);
    });

    it.each(["history", "submit"])(
      "does not echo failed notary %s diagnostics",
      (operation) => {
        const result = fixture().run("notarize-macos.sh", {
          ...configuration,
          MOCK_FAIL_NOTARY: operation,
        });
        expect(result.status).not.toBe(0);
        expect(result.calls).toContain(`xcrun notarytool ${operation} `);
        expect(result.output).not.toContain("synthetic-secret-diagnostic");
      },
    );

    it("checks the combined command's configuration before starting a build", () => {
      const result = fixture().run("build-and-notarize-macos.sh", {});
      expect(result.status).not.toBe(0);
      expect(result.output).toContain("APPLE_SIGNING_IDENTITY");
      expect(result.calls).not.toMatch(/(?:pnpm|security|xcrun|codesign) /);
    });
  },
);
