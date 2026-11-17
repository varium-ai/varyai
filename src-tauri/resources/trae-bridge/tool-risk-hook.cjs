"use strict";
// Read-only PreToolUse observation. Never emit a permission decision or tool input.
const fs = require("node:fs");
const http = require("node:http");

function project(value) {
  if (
    value?.hook_event_name !== "PreToolUse" ||
    value.tool_name !== "RunCommand" ||
    !value.tool_input ||
    typeof value.tool_input.command !== "string"
  )
    return null;
  return {
    hook_event_name: value.hook_event_name,
    tool_name: value.tool_name,
    tool_input: { command: value.tool_input.command },
    session_id: value.session_id,
    tool_use_id: value.tool_use_id,
  };
}

async function run(configPath, input = process.stdin) {
  let request;
  const deadline = setTimeout(() => {
    request?.destroy();
    input.destroy();
  }, 750);
  try {
    const metadata = fs.lstatSync(configPath);
    if (
      !metadata.isFile() ||
      metadata.isSymbolicLink() ||
      metadata.size > 16384
    )
      return;
    const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
    const url = new URL(config.url);
    if (
      url.protocol !== "http:" ||
      url.hostname !== "127.0.0.1" ||
      !["/private/tool-risk/trae", "/private/tool-risk/trae-intl"].includes(
        url.pathname,
      ) ||
      url.search ||
      url.hash ||
      url.username ||
      url.password ||
      typeof config.token !== "string" ||
      !/^[a-f0-9]{64}$/.test(config.token)
    )
      return;
    const chunks = [];
    let size = 0;
    for await (const chunk of input) {
      size += Buffer.byteLength(chunk);
      if (size > 256 * 1024) return;
      chunks.push(Buffer.from(chunk));
    }
    const value = project(JSON.parse(Buffer.concat(chunks).toString("utf8")));
    if (!value) return;
    const body = JSON.stringify(value);
    await new Promise((resolve) => {
      request = http.request(
        url,
        {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "content-length": Buffer.byteLength(body),
            authorization: "Bearer " + config.token,
          },
        },
        (response) => {
          response.resume();
          response.on("end", resolve);
          response.on("error", resolve);
        },
      );
      request.on("error", resolve);
      request.end(body);
    });
  } catch {
    // Observation failure must not change native tool execution or expose its input.
  } finally {
    clearTimeout(deadline);
    request?.destroy();
  }
}

module.exports = { project, run };
if (require.main === module) run(process.argv[2]);
