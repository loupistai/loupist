# Interface localization

English JSON is the source of truth. Bundled namespaces are `common`, `app`,
`chat`, `knowledge`, `settings`, `connections`, `briefings`, `onboarding`, `assets`, and `sharing`.
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

Korean interface copy uses the formal register of a professional macOS tool,
never 해요체: a casual-polite ending lowers trust in financial research.
- Statements end in …습니다 / …입니다 ("아직 알림이 없습니다").
- Instructions use …하십시오 / …해 주십시오 ("연결을 확인한 뒤 다시 시도하십시오"),
  or a plain statement or noun phrase when that reads as naturally and is no longer.
- Questions and confirmations use …하시겠습니까?
- Buttons, menu items, labels, headers, chips and tabs are nouns or verb stems
  ("추가", "다시 시도", "연결 해제"); status and progress use a noun or …하는 중
  ("확인 중", "답변 작성 중").
- No …요 ending of any kind (…해요, …예요, …세요, …주세요, …까요, …죠), no exclamation
  marks, emoji or softeners ("살짝", "잠깐만요"), and no bureaucratese
  (…하시기 바랍니다, stacked honorifics). Keep it short, exact and calm, about as
  long as the English, with product terms (봇, 리서치 세션, 관심 자산, 월드, 렌즈, 예약,
  자료함, 루피스트) unchanged.
- Text the user sends to their Loupist (a suggested prompt, an inserted draft) is
  the user's own plain request: "…해 줘".

`scripts/check-ui-copy.mjs` fails on a Korean string that ends a sentence in …요;
a word that only ends in 요 (필요, 중요, 주요, 개요, 수요, 소요) and the answer
아니요 (paired with 예, as on macOS) are allowed.

A Korean particle after a value is written with both forms — `{{name}}이(가)`,
`을(를)`, `은(는)`, `과(와)`, `아(야)`, `(으)로` — and the `koreanParticles`
post-processor (`koreanParticles.ts`) picks one by the final sound of the text
before it: Hangul by its final consonant (ㄹ takes 로), digits by their Korean
reading, Latin names by a loanword rule; an unknown sound keeps the pair. Reword
instead when a string reads better without a particle. `scripts/check-ui-copy.mjs`
rejects a single bare particle after a value (`{{name}}을`) and a wrong one after
the app's name (루피스트가, 루피스트를).

World relation names are stored as English SCREAMING_SNAKE (`FILES_WITH`).
`knowledge.relations` labels the reviewed types; `knowledge.customRelations` is the
bundled table of common open relations the extraction model names itself, keyed the
same way. Extend it by adding the stored name to the English catalog and every
locale, in the short noun style of the reviewed labels ("공시 제출처", "생산지"). A
name in neither table is shown as lowercase English; see
[World labels](../../../../docs/ui/map-lens.md#world-and-library).

`useLanguage` shares the global appearance configuration reader with Theme.
Rust validates and persists `appearance.language = "system" | "en" | "ko"` and
`appearance.price_colors = "auto" | "red-up" | "green-up"` (see
[desktop design system](../../../../docs/dev/ui-qa.md#desktop-design-system)).
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
