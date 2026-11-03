# Interface localization

English JSON is the source of truth. Bundled namespaces are `common`, `app`,
`chat`, `knowledge`, `settings`, `connections`, `briefings`, and `onboarding`.
Use namespace-relative nested keys: `useTranslation("settings")` returns a typed
`t`, and `t("appearance.title")` reads `locales/en/settings.json` → `appearance.title`.
Cross-namespace i18next keys use a colon, such as `settings:appearance.title`.
TypeScript resources are inferred from the English catalogs; unknown keys fail
compilation, including calls that supply a default value.

Import `useTranslation` and `Trans` from this directory. Keep user content,
source text, Lens documents, model identifiers, and native language names out of
translation catalogs. Korean catalogs are translated; the parity check requires
matching keys and placeholders and `_status: "translated"` for locale metadata.
Missing localized keys fall back to English.

A Korean particle after a value is written with both forms — `{{name}}이(가)`,
`을(를)`, `은(는)`, `과(와)`, `아(야)`, `(으)로` — and the `koreanParticles`
post-processor (`koreanParticles.ts`) picks one by the final sound of the text
before it: Hangul by its final consonant (ㄹ takes 로), digits by their Korean
reading, Latin names by a loanword rule; an unknown sound keeps the pair. Reword
instead when a string reads better without a particle.

`useLanguage` shares the global appearance configuration reader with Theme.
Rust validates and persists `appearance.language = "system" | "en" | "ko"`.
System follows the first supported language in `navigator.languages`, falling
back to English. Saved changes and OS language events update the document and
subscribed components without restart. Development offers the transient `en-XA`
pseudo-locale; it is never saved to configuration. Preview accepts
`locale=en|ko|en-XA` and begins with that locale regardless of OS preferences.

`format.ts` uses the active locale and caches Intl formatters by locale/options.
`formatPercent` takes a fraction;
`formatDate`/`formatDateTime` accept Date objects or epoch milliseconds;
`formatRelative` takes a signed offset and Intl time unit. Date options can
specify a time zone when the product context requires one. Components displaying
formatted values should subscribe with `useTranslation` so language changes
rerender them.

Run `node scripts/check-i18n.mjs --report` from the app directory to inventory
remaining literals. `--enforce` fails for any unallowlisted finding. The AST
check covers JSX text, selected accessibility/display attributes, and known
error/toast helpers; strings in variable initializers and templates still need
review. Allowlist entries require an app-relative `file`, exact normalized
`text`, and a specific `reason`.
