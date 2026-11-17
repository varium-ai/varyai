// VaryAI Harness session identity helper v1. No histories or credentials are read.
const HEADER = "x-vary-harness-session-id";
export const name = "varyai-harness-session-identity";

export function apply(ctx) {
  ctx.on("llm-pi-ai/request-headers", (request, _headers, next) => {
    // Preserve other contributors' results and intentional failures.
    const headers = next();
    try {
      const url = new URL(request.baseURL);
      const id = request.sessionId;
      if (
        ["http:", "https:"].includes(url.protocol) &&
        ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname) &&
        !url.username &&
        !url.password &&
        !url.search &&
        !url.hash &&
        /^\/connections\/deepseek-harness(?:\/v1)?\/?$/.test(url.pathname) &&
        typeof id === "string" &&
        id.length > 0 &&
        id.length <= 200 &&
        /^[\x21-\x7e]+$/.test(id)
      )
        headers.set(HEADER, id);
    } catch {
      // Correlation failure never changes request success.
    }
    return headers;
  });
}
