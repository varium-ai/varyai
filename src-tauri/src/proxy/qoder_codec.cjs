// Load only the known protocol binding from the user's installed Qoder CN SDK.
// No SDK/WASM bytes or account credentials are bundled with Vary AI.
const fs = require("node:fs");
const crypto = require("node:crypto");
function main() {
  const input = JSON.parse(fs.readFileSync(0, "utf8"));
  if (input.operation === "trust") {
    const ca = new crypto.X509Certificate(input.ca);
    const tls = require("node:tls");
    if (typeof tls.getCACertificates !== "function") throw new Error();
    return {
      trusted: tls.getCACertificates("system").some((pem) => {
        const root = new crypto.X509Certificate(pem);
        return (
          root.ca &&
          root.subject === root.issuer &&
          root.verify(root.publicKey) &&
          root.fingerprint256 === ca.fingerprint256
        );
      }),
    };
  }
  const source = fs.readFileSync(process.env.VARYAI_QODER_WORKER, "utf8");
  const bindings = {
    b1494b39ecb713e06d3c8fb9b4aab9dff571942557d750be33870d06eb8e8b5a: {
      start: "var jic,",
      end: "uii=F(",
      xor: "_$d",
      salt: "qInfTBqB6D3R",
      exports: "dn",
      once: "S",
      bootstrap:
        'Aii();cii();aoc({module:Buffer.from(jic,"base64")});module.exports=aii;',
    },
    "76936c0212abd18ac762f2ab69d2449ef426bc48c5685a87bf2bab7653f7713b": {
      start: "var Yel,",
      end: "g1i=",
      xor: "_$d1",
      salt: "nj1pvEdtIHlt",
      exports: "Qt",
      once: "w",
      bootstrap:
        'n1i();u1i();sAl({module:Buffer.from(Yel,"base64")});module.exports=l1i;',
    },
  };
  const binding =
    bindings[crypto.createHash("sha256").update(source).digest("hex")];
  if (!binding) throw new Error();
  if (input.operation === "check") return { supported: true };
  const start = source.indexOf(binding.start);
  const end = source.indexOf(binding.end, start);
  if (start < 0 || end < start) throw new Error();
  const block = source
    .slice(start, end)
    .replace(/,\s*$/, ";")
    .replaceAll(
      "import.meta.url",
      'require("node:url").pathToFileURL(__filename).href',
    );
  const pre = `const ${binding.xor}=(s,k=${JSON.stringify(binding.salt)})=>{const b=Buffer.from(s,"base64");for(let i=0;i<b.length;i++)b[i]^=k.charCodeAt(i%k.length);return b.toString()};
    const ${binding.exports}=(o,p)=>{for(const [k,get] of Object.entries(p))Object.defineProperty(o,k,{get});};
    const ${binding.once}=f=>{let done=false;return()=>{if(!done){done=true;f();}}};`;
  const mod = { exports: {} };
  new Function(
    "require",
    "module",
    "__filename",
    pre + block + "\n" + binding.bootstrap,
  )(require, mod, process.env.VARYAI_QODER_WORKER);
  const sdk = mod.exports;
  globalThis.crypto ??= crypto.webcrypto;
  if (input.operation === "decode") {
    return {
      body: JSON.parse(
        sdk.decrypt_server_response(
          Buffer.from(input.body, "base64").toString("utf8"),
        ),
      ),
    };
  }
  if (input.operation !== "encode" || !input.body || Array.isArray(input.body))
    throw new Error();
  const h = Object.fromEntries(
    Object.entries(input.headers).map(([k, v]) => [k.toLowerCase(), v]),
  );
  if (!h.authorization?.startsWith("Bearer ")) throw new Error();
  const auth = JSON.parse(
    Buffer.from(h.authorization.slice(7).split(".")[1], "base64url").toString(),
  );
  if (typeof auth.info !== "string") throw new Error();
  const ctx = new sdk.QoderContext(
    h["cosy-machineid"] || "",
    h["cosy-version"] || "",
    JSON.stringify({
      uid: h["cosy-user"] || "",
      key: h["cosy-key"] || "",
      encrypt_user_info: auth.info,
    }),
    JSON.stringify({
      client_type: h["cosy-clienttype"],
      business_product: h["cosy-business-product"],
      business_type: h["cosy-business-type"],
      scene: h["cosy-scene"],
    }),
  );
  let request;
  try {
    request = ctx.prepareInferRequest(
      "https://gateway.qoder.com.cn",
      JSON.stringify(input.body),
      input.body.model_config?.key,
      input.body.model_config?.source,
    );
    const headers = Object.fromEntries(request.headers);
    return {
      body: Buffer.from(request.body).toString("base64"),
      authorization: headers.Authorization,
      date: headers["Cosy-Date"],
    };
  } finally {
    request?.free();
    ctx.free();
  }
}
try {
  process.stdout.write(JSON.stringify(main()));
} catch {
  process.stdout.write(JSON.stringify({ error: "qoder_codec_unavailable" }));
  process.exitCode = 1;
}
// Allow stdout to drain: process.exit() truncates large encoded requests.
