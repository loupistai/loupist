// React hook rules only. TypeScript, formatting and style stay with tsc and review.
import tsParser from "@typescript-eslint/parser";
import reactHooks from "eslint-plugin-react-hooks";

export default [
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsParser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    linterOptions: { reportUnusedDisableDirectives: "error" },
    plugins: { "react-hooks": reactHooks },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      // `npm run lint:hooks` allows no warnings; a deliberate omission carries a disable with its reason.
      "react-hooks/exhaustive-deps": "warn",
    },
  },
];
