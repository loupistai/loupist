// In-memory World vocabulary store for the mock bridge and previews.

import type { ExtensionType, SchemaChange, SchemaChangeProducer, SchemaRemovalKind, WorldExtension, WorldSchemaView } from "../../contracts/knowledge";

const MAX_TYPES = 4;
const MAX_RELATIONS = 6;
const MAX_CHANGES = 20;

function emptyWorldSchema(now = 0): WorldSchemaView {
  return { extension: { version: 0, types: [], relations: [], updatedAtMs: now }, changes: [] };
}

/** The preset a template starts with, whatever the focus text says (Rust `curated_seed`). */
export function curatedSeedView(template: string | undefined, now = 0): WorldSchemaView | null {
  const extension = template === "semiconductors"
    ? semiconductorExtension(now)
    : template === "dividends"
      ? dividendExtension(now)
      : template === "macro"
        ? macroExtension(now)
        : null;
  return extension ? changed(emptyWorldSchema(now), extension, "preset", now) : null;
}

function names(extension: WorldExtension) {
  return { types: extension.types.map(type => type.name), relations: extension.relations.map(relation => relation.name) };
}

/** The view after `next` replaces the live extension, with one more change recorded. */
function changed(view: WorldSchemaView, next: WorldExtension, producer: SchemaChangeProducer, now: number, changeId = `change-${view.changes.length + 1}`): WorldSchemaView {
  const before = names(view.extension);
  const after = names(next);
  const diff = (from: string[], to: string[]) => to.filter(name => !from.includes(name));
  const addedTypes = diff(before.types, after.types);
  const addedRelations = diff(before.relations, after.relations);
  const removedTypes = diff(after.types, before.types);
  const removedRelations = diff(after.relations, before.relations);
  const labelFor = (name: string) => {
    const found = [...next.types, ...view.extension.types].find(type => type.name === name) ?? [...next.relations, ...view.extension.relations].find(relation => relation.name === name);
    return found ? { name, labels: found.labels } : null;
  };
  const kindLabels = [...addedTypes, ...addedRelations, ...removedTypes, ...removedRelations].flatMap(name => labelFor(name) ?? []);
  const list = (kinds: string[]) => kindLabels.filter(kind => kinds.includes(kind.name)).map(kind => kind.labels.en).join(", ");
  const summary = [addedTypes.length + addedRelations.length ? `Added ${list([...addedTypes, ...addedRelations])}` : "", removedTypes.length + removedRelations.length ? `Removed ${list([...removedTypes, ...removedRelations])}` : ""].filter(Boolean).join(". ");
  const change: SchemaChange = { changeId, producer, summary, addedTypes, addedRelations, removedTypes, removedRelations, kindLabels, createdAtMs: now, undoable: true };
  const extension = { ...next, version: view.extension.version + 1, updatedAtMs: now };
  return refreshUndoable({ extension, changes: [change, ...view.changes].slice(0, MAX_CHANGES) });
}

/** A change is undoable while one of its added kinds is still tracked or one of its removed kinds is gone. */
function refreshUndoable(view: WorldSchemaView): WorldSchemaView {
  const live = names(view.extension);
  return {
    ...view,
    changes: view.changes.map(change => ({
      ...change,
      undoable: change.addedTypes.some(name => live.types.includes(name)) || change.addedRelations.some(name => live.relations.includes(name))
        || change.removedTypes.some(name => !live.types.includes(name)) || change.removedRelations.some(name => !live.relations.includes(name)),
    })),
  };
}

function withoutType(extension: WorldExtension, name: string): WorldExtension {
  const relations = extension.relations.flatMap(relation => {
    const signatures = relation.signatures.filter(([source, target]) => source !== name && target !== name);
    return signatures.length ? [{ ...relation, signatures }] : [];
  });
  return { ...extension, types: extension.types.filter(type => type.name !== name), relations };
}

function semiconductorExtension(now: number): WorldExtension {
  const processNode: ExtensionType = {
    name: "ProcessNode",
    description: "A named semiconductor manufacturing process generation, such as N3, 18A or 1b DRAM. Not the fab that runs it (Place) and not the chip made on it (Product).",
    category: "product",
    labels: { en: "Process node", ko: "공정 노드" },
    blurbs: { en: "Process generations such as N3 or 18A", ko: "N3, 18A 같은 반도체 공정 세대" },
    fields: [],
  };
  return {
    version: 1,
    updatedAtMs: now,
    types: [processNode],
    relations: [
      {
        name: "QUALIFIED_BY",
        description: "The target customer has qualified or certified the source product for use. Keep the date and stage in the fact.",
        signatures: [["Product", "Organization"]],
        labels: { en: "qualified by", ko: "인증" },
      },
      {
        name: "MADE_ON",
        description: "The source product is manufactured on the target process node.",
        signatures: [["Product", "ProcessNode"]],
        labels: { en: "made on", ko: "제조 공정" },
      },
    ],
  };
}

function dividendExtension(now: number): WorldExtension {
  return {
    version: 1,
    updatedAtMs: now,
    types: [{
      name: "CapitalReturnPolicy",
      description: "A company's stated policy for returning cash to shareholders: a payout target, a progressive dividend commitment or a buyback authorization. A single dividend declaration is an Event, and a paid amount is a Metric.",
      category: "event",
      labels: { en: "Capital return policy", ko: "주주환원 정책" },
      blurbs: { en: "Dividend and buyback policies", ko: "배당·자사주 매입 같은 주주환원 정책" },
      fields: [{ name: "payout_target", description: "The stated payout ratio or dividend target, as written." }],
    }],
    relations: [{
      name: "HAS_RETURN_POLICY",
      description: "The organization has adopted or announced the target capital return policy.",
      signatures: [["Organization", "CapitalReturnPolicy"]],
      labels: { en: "has return policy", ko: "환원 정책" },
    }],
  };
}

function macroExtension(now: number): WorldExtension {
  return {
    version: 1,
    updatedAtMs: now,
    types: [{
      name: "EconomicIndicator",
      description: "A published economic data series such as CPI, payrolls or GDP. One release of it is an Event, and one value is a Metric.",
      category: "event",
      labels: { en: "Economic indicator", ko: "경제 지표" },
      blurbs: { en: "Published series such as CPI or payrolls", ko: "소비자물가, 고용 같은 발표 지표" },
      fields: [],
    }],
    relations: [
      { name: "SETS_POLICY_FOR", description: "The source central bank or authority sets monetary or fiscal policy for the target economy.", signatures: [["Organization", "Place"]], labels: { en: "sets policy for", ko: "정책 결정" } },
      { name: "PUBLISHES", description: "The source agency or central bank publishes the target economic indicator.", signatures: [["Organization", "EconomicIndicator"]], labels: { en: "publishes", ko: "발표" } },
      { name: "MEASURES", description: "The source indicator measures the target economy or market.", signatures: [["EconomicIndicator", "Place"], ["EconomicIndicator", "Asset"]], labels: { en: "measures", ko: "측정 대상" } },
      { name: "MOVES_WITH", description: "The source asset has moved with the target indicator or asset, only as a source states it. Keep the direction and the period in the fact.", signatures: [["Asset", "EconomicIndicator"], ["Asset", "Asset"]], labels: { en: "moves with", ko: "연동" } },
    ],
  };
}

/** Semiconductors preset plus a later background-review change, for the tracking previews. */
export function semiconductorTrackingView(now = 0): WorldSchemaView {
  const preset = curatedSeedView("semiconductors", now)!;
  const capacityPlan: ExtensionType = {
    name: "CapacityPlan",
    description: "A stated plan to add manufacturing capacity, including the site, the process it expands, and when the new volume is expected to start.",
    category: "event",
    labels: { en: "Capacity plan", ko: "증설 계획" },
    blurbs: { en: "Announced plans to add production capacity", ko: "생산능력을 늘리는 증설 계획" },
    fields: [],
  };
  return changed(preset, { ...preset.extension, types: [...preset.extension.types, capacityPlan] }, "review", now + 1, "change-review");
}

export class MockWorldSchema {
  private readonly views = new Map<string, WorldSchemaView>();
  private readonly handlers = new Set<(event: { loupistId: string }) => void>();

  install(loupistId: string, view: WorldSchemaView) {
    this.views.set(loupistId, structuredClone(view));
  }

  get(loupistId: string): WorldSchemaView {
    return structuredClone(this.views.get(loupistId) ?? emptyWorldSchema());
  }

  undo(loupistId: string, changeId: string): WorldSchemaView {
    const current = this.views.get(loupistId) ?? emptyWorldSchema();
    const change = current.changes.find(item => item.changeId === changeId);
    if (!change?.undoable) throw new Error("That change can't be undone.");
    let next = current.extension;
    for (const name of change.addedRelations) next = { ...next, relations: next.relations.filter(relation => relation.name !== name) };
    for (const name of change.addedTypes) next = withoutType(next, name);
    return this.commit(loupistId, changed(current, next, "undo", Date.now(), `change-${current.changes.length + 1}-undo`));
  }

  remove(loupistId: string, kind: SchemaRemovalKind, name: string): WorldSchemaView {
    const current = this.views.get(loupistId) ?? emptyWorldSchema();
    let next = current.extension;
    if (kind === "type") {
      if (!next.types.some(type => type.name === name)) throw new Error("That type is not tracked.");
      next = withoutType(next, name);
    } else {
      if (!next.relations.some(relation => relation.name === name)) throw new Error("That relationship is not tracked.");
      next = { ...next, relations: next.relations.filter(relation => relation.name !== name) };
    }
    return this.commit(loupistId, changed(current, next, "undo", Date.now(), `change-${current.changes.length + 1}-remove`));
  }

  subscribe(handler: (event: { loupistId: string }) => void) {
    this.handlers.add(handler);
    return () => { this.handlers.delete(handler); };
  }

  private commit(loupistId: string, view: WorldSchemaView) {
    if (view.extension.types.length > MAX_TYPES || view.extension.relations.length > MAX_RELATIONS) throw new Error("This map is already tracking as many kinds as it can.");
    const stored = structuredClone(view);
    this.views.set(loupistId, stored);
    for (const handler of this.handlers) handler({ loupistId });
    return structuredClone(stored);
  }
}
