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
