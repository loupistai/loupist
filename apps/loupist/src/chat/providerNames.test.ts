import { FEATURED_KEY_ROWS, SIGN_IN_PROVIDERS, isUpstageEndpoint } from "./providerManifest";
import { providerKeyName, providerShortName } from "./providerNames";

it("names providers the way people know them in sentences", () => {
  expect(providerShortName("openai")).toBe("ChatGPT");
  expect(providerShortName("openai", "api_key")).toBe("OpenAI");
  expect(providerShortName("anthropic")).toBe("Anthropic");
  expect(providerShortName("anthropic", "api_key")).toBe("Anthropic");
  expect(providerShortName("custom-local")).toBeUndefined();
  expect(providerKeyName("xai", "xAI API")).toBe("xAI");
  expect(providerKeyName("groq", "Groq")).toBe("Groq");
});

it("features the manifest's sign-ins and key rows in their declared order", () => {
  expect(SIGN_IN_PROVIDERS).toEqual(["openai", "xai"]);
  expect(FEATURED_KEY_ROWS).toEqual(["anthropic", "openai", "google", "xai", "upstage", "openrouter"]);
  expect(providerShortName("xai")).toBe("SuperGrok");
  expect(providerKeyName("upstage", "Upstage")).toBe("Upstage Solar");
  expect(isUpstageEndpoint("https://api.upstage.ai/v1")).toBe(true);
  expect(isUpstageEndpoint("http://localhost:11434/v1")).toBe(false);
});
