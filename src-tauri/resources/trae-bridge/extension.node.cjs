const { test } = require("node:test");
const assert = require("node:assert/strict");
const { validAuditUrl } = require("./extension.cjs");
test("audit transport accepts only the fixed loopback endpoint", () => {
  assert.equal(
    validAuditUrl("http://127.0.0.1:15721/private/trae/audit"),
    true,
  );
  for (const value of [
    "https://evil.test/private/trae/audit",
    "http://127.0.0.1.evil.test/private/trae/audit",
    "http://user@127.0.0.1:15721/private/trae/audit",
    "http://127.0.0.1:15721/private/trae/audit?token=secret",
  ])
    assert.equal(validAuditUrl(value), false);
});
