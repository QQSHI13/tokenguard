// Shared provider types and one-click presets, used by both the Providers
// tab and the onboarding flow.

export type ProviderFormat = "openai" | "anthropic" | "google";
export type AuthScheme = "bearer" | "x_api_key" | "api_key" | "x_goog_api_key";

export type ModelMapping = {
  local: string;
  remote: string;
  input_cost_per_1k: number | null;
  output_cost_per_1k: number | null;
  cached_input_cost_per_1k: number | null;
};

export type ProviderPreset = {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  models: ModelMapping[];
};

const noCost = {
  input_cost_per_1k: null,
  output_cost_per_1k: null,
  cached_input_cost_per_1k: null,
};

export const PRESETS: ProviderPreset[] = [
  {
    name: "OpenAI",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    models: [
      { local: "gpt-4o", remote: "gpt-4o", ...noCost },
      { local: "gpt-4o-mini", remote: "gpt-4o-mini", ...noCost },
      { local: "gpt-4-turbo", remote: "gpt-4-turbo", ...noCost },
      { local: "gpt-3.5-turbo", remote: "gpt-3.5-turbo", ...noCost },
    ],
  },
  {
    name: "Anthropic",
    base_url: "https://api.anthropic.com",
    format: "anthropic",
    auth: "x_api_key",
    models: [
      { local: "claude-sonnet-4", remote: "claude-sonnet-4-20250514", ...noCost },
      { local: "claude-3-5-sonnet", remote: "claude-3-5-sonnet-20241022", ...noCost },
      { local: "claude-3-5-haiku", remote: "claude-3-5-haiku-20241022", ...noCost },
    ],
  },
  {
    name: "Google",
    base_url: "https://generativelanguage.googleapis.com",
    format: "google",
    auth: "x_goog_api_key",
    models: [
      { local: "gemini-1.5-pro", remote: "gemini-1.5-pro", ...noCost },
      { local: "gemini-1.5-flash", remote: "gemini-1.5-flash", ...noCost },
      { local: "gemini-2.0-flash", remote: "gemini-2.0-flash", ...noCost },
    ],
  },
  {
    name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
    format: "openai",
    auth: "bearer",
    models: [],
  },
];

/** Default auth header for a given API format. */
export function defaultAuthFor(format: ProviderFormat): AuthScheme {
  switch (format) {
    case "anthropic":
      return "x_api_key";
    case "google":
      return "x_goog_api_key";
    default:
      return "bearer";
  }
}
