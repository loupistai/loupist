const catalogs = import.meta.glob<Record<string, unknown>>("./locales/*/*.json", { eager: true, import: "default" });

const strings = (value: unknown, path: string[] = []): Array<[string, string]> =>
  typeof value === "string" ? [[path.join("."), value]]
    : value && typeof value === "object" ? Object.entries(value).flatMap(([key, child]) => strings(child, [...path, key])) : [];

const banned = { en: /ontolog|schema|entity types?/iu, ko: /온톨로지|스키마|엔티티\s*타입/u };

describe.each(["en", "ko"] as const)("%s interface copy", locale => {
  it("never says ontology, schema or entity type", () => {
    const own = Object.entries(catalogs).filter(([file]) => file.startsWith(`./locales/${locale}/`));
    expect(own.length).toBeGreaterThan(5);
    const found = own.flatMap(([file, catalog]) => strings(catalog, [file]).filter(([, text]) => banned[locale].test(text)).map(([key]) => key));
    expect(found).toEqual([]);
  });
});
