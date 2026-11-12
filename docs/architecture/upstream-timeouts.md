# Upstream model request timeouts

Model forwarding separates the wait for response headers from the lifetime of
the response body. Shared routing waits up to 600 seconds for upstream response
headers. Once headers arrive, a streaming response has no fixed total lifetime:
downstream cancellation, authorization revocation and transport errors can still
end it. Existing routing first-content and retry deadlines remain effective and
may impose a shorter wait. Nonstreaming shared requests retain their 600-second
total deadline.

The forwarding clients retain the existing proxy configuration, 30-second
connection timeout, decompression policy and redirect restrictions. Auxiliary
clients used for authentication, model discovery and configuration checks retain
their existing total deadlines. Other provider forwarding paths retain their
existing timeout controls; this change applies to shared routing model requests.

This follows Magpie's distinction between a response-header deadline and a
stream's lifetime, reviewed in
[`internal/gateway/upstream_transport.go`](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/upstream_transport.go)
and the
[`TRAE plugin`](https://github.com/magpie-community/plugins/blob/d677c436d30b3682508d2170bda70572a6c06aa8/packages/trae/index.mjs).
The implementation is independent; no reference source is copied.

A trace ending at approximately 600 seconds after an upstream HTTP 200 can
indicate the previous total request deadline. TRAE's generic model error 3003
and its `x-trae-empty-sse:true` AgentRun error do not, on their own, establish
that this deadline caused a particular failure. The change prevents an open
model stream from being cut off solely because its total duration exceeds the
header wait. It does not resolve upstream stalls or promise live TRAE acceptance.

Regression tests use dynamic loopback ports and a short header deadline. They
verify rejection before headers, continued body reads after that deadline,
redirect restrictions and cancellation of a pending header wait.
