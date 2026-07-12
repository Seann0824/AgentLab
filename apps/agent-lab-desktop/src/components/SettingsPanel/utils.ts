import type { ProviderConfig } from "../../types/chat";

export const PROVIDER_OPTIONS = ["DeepSeek", "OpenAI", "OpenRouter", "Custom"];

export function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

export function emptyProvider(): ProviderConfig {
  return {
    id: generateId(),
    name: "",
    provider: "Custom",
    base_url: "",
    api_key: "",
    models: [],
  };
}
