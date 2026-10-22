import { WorkBuddyProviderForm } from "./WorkBuddyProviderForm";
import type { ProviderFormProps } from "./ProviderForm";

/** Shared desktop model editor; Qoder keeps its native provider/model schema. */
export function QoderProviderForm(props: ProviderFormProps) {
  const original = props.initialData?.settingsConfig ?? {};
  const nativeModels: any[] = Array.isArray(original.models)
    ? original.models
    : [];
  const first =
    nativeModels.find((m: any) => m.model === original.model) ??
    nativeModels[0];
  const initialData = props.initialData
    ? {
        ...props.initialData,
        settingsConfig: {
          ...original,
          id: original.model ?? "",
          url: original.baseUrl ?? "",
          models: nativeModels.map((m: any) => ({
            ...m,
            id: m.model,
            name: m.displayName,
          })),
          maxInputTokens: first?.contextWindow,
          maxOutputTokens: first?.maxOutputTokens,
          supportsImages: first?.capabilities?.vision,
          supportsToolCall: true,
        },
      }
    : undefined;
  const convertAndSubmit = async (
    data: Parameters<ProviderFormProps["onSubmit"]>[0],
    submit: ProviderFormProps["onSubmit"],
  ) => {
    if (data.presetCategory === "official") {
      await submit(data);
      return;
    }
    const form = JSON.parse(data.settingsConfig);
    const models = (form.models ?? [{ id: form.id, name: data.name }]).map(
      (m: any) => {
        const previous =
          nativeModels.find((old: any) => old.model === m.id) ?? {};
        return {
          ...previous,
          model: m.id,
          displayName: m.name ?? m.id,
          contextWindow:
            form.maxInputTokens === first?.contextWindow
              ? (previous.contextWindow ?? form.maxInputTokens ?? 200000)
              : (form.maxInputTokens ?? 200000),
          maxOutputTokens:
            form.maxOutputTokens === first?.maxOutputTokens
              ? (previous.maxOutputTokens ?? form.maxOutputTokens ?? 8192)
              : (form.maxOutputTokens ?? 8192),
          capabilities: {
            ...previous.capabilities,
            vision:
              form.supportsImages === (first?.capabilities?.vision === true)
                ? (previous.capabilities?.vision ??
                  form.supportsImages === true)
                : form.supportsImages === true,
            thinking: previous.capabilities?.thinking ?? {
              modes: [],
              supportsEffort: false,
              supportedEffortLevels: [],
            },
          },
        };
      },
    );
    const endpoint = new URL(form.url);
    endpoint.pathname =
      endpoint.pathname
        .replace(/\/chat\/completions\/?$/, "")
        .replace(/\/$/, "") || "/v1";
    const config = {
      ...original,
      baseUrl: endpoint.toString(),
      apiKey: form.apiKey,
      type: "openai-compatible",
      protocol: form.protocol ?? "openai",
      authType:
        form.protocol === original.protocol && original.authType
          ? original.authType
          : form.protocol === "anthropic"
            ? "api-key"
            : "bearer",
      model: form.id,
      models,
    };
    await submit({
      ...data,
      settingsConfig: JSON.stringify(config, null, 2),
    });
  };
  const addThirdParty = props.onAddThirdParty;
  return (
    <WorkBuddyProviderForm
      {...props}
      desktopClient="qoder"
      initialData={initialData}
      onSubmit={(data) => convertAndSubmit(data, props.onSubmit)}
      onAddThirdParty={
        addThirdParty
          ? (data) => convertAndSubmit(data, addThirdParty)
          : undefined
      }
    />
  );
}
