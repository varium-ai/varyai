# Keep Vary AI loopback traffic local

Use these examples only after confirming that a VPN/proxy is forwarding requests for `127.0.0.1:15721` to a remote proxy. HTTP 503 alone does not establish that diagnosis.

The examples contain no subscription, server, account, or key. They route loopback traffic directly and send other traffic to the user's selected proxy. They replace the routing policy; they do not preserve an existing region-based split-routing policy. LAN addresses are not included in the direct exception.

## V2RayXS

Import [the routing JSON](v2rayxs-vary-local-direct.json) through the client's JSON configuration-file importer. Save the configuration, then select the `Vary_Local_Direct` routing set and the applicable system-proxy mode. Retain your existing proxy server.

V2RayXS 1.6.3 supports importing a JSON object containing only `routing`. Selecting the imported rules is a separate step; repeated imports may create duplicate sets. `main` means the selected proxy node and `direct` means a direct connection. The first rules match `127.0.0.0/8`, `::1/128`, and `localhost`.

Use a single client to manage the system proxy. TUN is not required for this example.

## Clash Verge / Mihomo

[The YAML snippet](clash-verge-vary-local-direct.yaml) is a Merge configuration for an existing subscription, not a complete subscription. Replace its `rules` array through the subscription's Merge/override editor, preserving unrelated settings. Check the generated configuration: it must contain the intended four rules in order.

In the client, select a working node or group for the built-in `GLOBAL` group, then switch to **Rule** mode. Do not select `DIRECT` as the fallback proxy. Native **Global** mode skips rule matching, so adding entries under `rules` does not provide this exception in that mode. The snippet deliberately does not set `mode`; the client's selected mode takes precedence.

The final `MATCH,GLOBAL` sends all remaining traffic through the selected group. To preserve an existing split-routing policy, insert only the three loopback rules at the start of that policy and keep Rule mode instead of replacing the entire rule set.

System-proxy bypass settings and `NO_PROXY` help only programs that respect them. They are not substitutes for verifying the proxy engine's effective routing rules.

## Verify

Keep the same upstream node, reload the requesting client, and test a normal request through Vary AI. Check that websites requiring the proxy still work. The proxy connection log should show loopback requests as `DIRECT`/`direct`, or show no engine connection if the requesting program bypassed the proxy itself.

## Implementation references

- [V2RayXS 1.6.3 importer](https://github.com/tzmax/V2RayXS/blob/ee0f5a7a/V2RayX/ConfigImporter.m)
- [Mihomo operation modes](https://wiki.metacubex.one/en/config/general/#operation-mode)
- [Clash Verge Merge implementation](https://github.com/clash-verge-rev/clash-verge-rev/blob/dev/src-tauri/src/enhance/merge.rs)

Client versions and menus can change. Inspect the effective configuration rather than relying only on an import-success message.
