// Only the local VaryAI gateway receives this native Pi session identity.
export default function registerPiSessionId(pi) {
  pi.on("before_provider_headers", (event, context) => {
    try {
      delete event.headers["x-vary-pi-session-id"];
      const url = new URL(context.model?.baseUrl);
      if (
        url.protocol !== "http:" ||
        !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname) ||
        url.username ||
        url.password ||
        !/^\/(?:connections\/pi|pi)(?:\/|$)/.test(url.pathname)
      )
        return;
      const id = context.sessionManager.getSessionId();
      if (
        typeof id !== "string" ||
        !id ||
        id.length > 256 ||
        /[\s\u0000-\u001f\u007f]/.test(id)
      )
        return;
      event.headers["x-vary-pi-session-id"] = id;
    } catch {
      // Missing session metadata must never prevent a provider request.
    }
  });
}
