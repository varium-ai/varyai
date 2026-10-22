import { invoke } from "@tauri-apps/api/core";

export interface ProviderRequest {
  requestId: string;
  name: string;
  websiteUrl: string;
  description: string;
}

export const providerRequestsApi = {
  submit: (request: ProviderRequest): Promise<void> =>
    invoke("submit_provider_request", { request }),
};
