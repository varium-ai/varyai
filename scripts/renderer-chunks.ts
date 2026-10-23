import path from "node:path";

const sourceRoot = `${path.resolve("src").replace(/\\/g, "/")}/`;

/**
 * Explicit, dependency-aligned chunks. Keep mutually dependent modules together:
 * provider settings share OAuth UI, and appConfig renders shared icon components.
 * The real-build regression checks byte budgets and static dependency cycles.
 */
export function rendererChunk(id: string): string | undefined {
  const file = id.replace(/\\/g, "/");
  // Shared helpers must not live in app chunks: vendor modules also import them.
  if (file.includes("commonjsHelpers") || file === "\0vite/preload-helper.js") {
    return "vendor-interop";
  }

  if (file.includes("/node_modules/")) {
    const parts = file
      .slice(file.lastIndexOf("/node_modules/") + 14)
      .split("/");
    const name = parts[0].startsWith("@")
      ? parts.slice(0, 2).join("/")
      : parts[0];
    if (["react", "react-dom", "scheduler"].includes(name))
      return "vendor-react";
    if (["@codemirror/state", "@lezer/common", "@lezer/lr"].includes(name)) {
      return "vendor-editor-core";
    }
    if (
      ["@codemirror/view", "style-mod", "w3c-keyname", "crelt"].includes(name)
    ) {
      return "vendor-editor-view";
    }
    if (
      name.startsWith("@codemirror/") ||
      name.startsWith("@lezer/") ||
      name === "codemirror"
    ) {
      return "vendor-editor-language";
    }
    if (
      name === "recharts" ||
      name.startsWith("d3-") ||
      ["victory-vendor", "es-toolkit", "eventemitter3"].includes(name)
    ) {
      return "vendor-charts";
    }
    if (name.startsWith("@radix-ui/")) return "vendor-radix";
    if (name === "framer-motion" || name.startsWith("motion-")) {
      return "vendor-motion";
    }
    if (name === "cmdk") return "vendor-command";
    return "vendor-base";
  }

  if (!file.startsWith(sourceRoot)) return undefined;
  const relative = file.slice(sourceRoot.length);
  if (relative.startsWith("i18n/locales/")) {
    return `locale-${path.basename(relative, ".json")}`;
  }
  // These exceptions depend on shared UI/helpers rather than only preset data.
  if (
    relative.startsWith("config/") &&
    !/^(?:config\/)(appConfig|codingPlanProviders|mcpPresets|universalProviderPresets)\.tsx?$/.test(
      relative,
    )
  ) {
    return "provider-presets";
  }
  if (relative === "utils/grokBuildConfig.ts") return "provider-presets";
  if (
    [
      "components/providers/forms/ConnectionPresetPicker.tsx",
      "components/providers/forms/ProviderConfigDetection.tsx",
      "components/providers/forms/shared/ModelDropdown.tsx",
    ].includes(relative)
  ) {
    return "ui-connection-presets";
  }
  if (relative.startsWith("icons/")) return "icons";
  if (relative.startsWith("components/")) {
    const parts = relative.slice("components/".length).split("/");
    if (parts.length > 1) {
      if (["providers", "settings", "proxy"].includes(parts[0])) {
        return "ui-provider-settings";
      }
      if (["ui", "deeplink"].includes(parts[0])) return "app-shared";
      return `ui-${parts[0]}`;
    }
    return "app-shared";
  }
  if (/^(hooks|lib|utils|types|i18n|config)\//.test(relative)) {
    return "app-shared";
  }
  return undefined;
}
