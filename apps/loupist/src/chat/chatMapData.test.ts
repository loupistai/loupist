import { addedMapItems, chatMapData, stepEntityIds } from "./chatMapData";
import type { GrowthEvent, KnowledgeMap } from "../contracts/knowledge";
import type { ChatMessage } from "../types";
const map = { nodes: ['a', 'b', 'c', 'd', 'unrelated'].map(id => ({ id, label: id, kind: 'entity', degree: 1, watched: false, held: false })), edges: [{ id: 'ab', kind: 'claim', source: 'a', target: 'b' }, { id: 'bc', kind: 'claim', source: 'b', target: 'c' }, { id: 'cd', kind: 'claim', source: 'c', target: 'd' }], worldRevisionId: 'rev' } as KnowledgeMap;
const event = (subjectId: string, kind: GrowthEvent['kind'] = 'entity_added', chat = 'chat'): GrowthEvent => ({ eventId: `${chat}:${kind}:${subjectId}`, loupistId: 'loupist', atMs: 1, kind, subjectId, originChatId: chat, originMessageId: 'answer', recordId: 'world', actor: 'loupist', summary: '', beforeSummary: null, afterSummary: null });
const answer: ChatMessage = { id: 'answer', role: 'assistant', content: '', created_at: '2026-10-28', status: 'complete' };
it('includes only this chat and exactly one hop, excluding another chat and two-hop neighbors', () => {
  const result = chatMapData(map, [], [event('a'), event('unrelated', 'entity_added', 'other')], 'chat');
  expect(result.nodes.map(node => node.id)).toEqual(['a', 'b']);
  expect(result.edges.map(edge => edge.id)).toEqual(['ab']);
});
it('seeds exact delivered and saved entity/claim references without treating whole Worlds or search hits as deliveries', () => {
  const reference = { kind: 'world_node' as const, recordId: 'world', revisionId: 'rev', objectId: 'a' };
  const messages: ChatMessage[] = [{ ...answer, memory_refs: [{ read_ref: 'ref', kind: 'world', title: 'A', excerpt: '', access: 'exact_read', knowledge: reference, as_of: null, section: null }, { read_ref: 'search', kind: 'world', title: 'Unrelated', excerpt: '', access: 'search_discovered', knowledge: { ...reference, objectId: 'unrelated' }, as_of: null, section: null }] }];
  expect(chatMapData(map, messages, [], 'chat').nodes.map(node => node.id)).toEqual(['a', 'b']);
  messages[0] = { ...answer, memory_update: { status: 'applied', commitId: 'commit', changes: [{ kind: 'World', operation: 'revise', recordId: 'world', title: 'claim', lesson: '', basis: '', futureUse: '', knowledge: { ...reference, kind: 'world_claim', objectId: 'bc' } }] } };
  expect(chatMapData(map, messages, [], 'chat').nodes.map(node => node.id)).toEqual(['a', 'b', 'c', 'd']);
});
it('deduplicates new items and highlights both endpoints of projected claims', () => {
  const projected = { ...map, edges: [...map.edges, { id: 'hyper:"d"', kind: 'claim' as const, source: 'a', target: 'd' }] };
  const additions = addedMapItems(projected, [event('a'), event('a'), event('hyper', 'claim_added'), event('b', 'claim_disputed')]);
  expect(additions.count).toBe(2);
  expect([...additions.highlightIds]).toEqual(['a', 'd']);
});
it('seeds names the host attributed to a step and lists them once in step order', () => {
  const running: ChatMessage = { ...answer, status: 'streaming', progress: { startedAtMs: 1, items: [
    { id: 'read-1', kind: 'tool', category: 'memory', operation: 'read', action: 'Read the map', status: 'succeeded', entityIds: ['c', 'a'], target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: 'note', kind: 'commentary', text: 'Checking' },
    { id: 'read-2', kind: 'tool', category: 'memory', operation: 'read', action: 'Read the map', status: 'running', entityIds: ['a', 'missing'], target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null } };
  expect(stepEntityIds(running)).toEqual(['c', 'a', 'missing']);
  expect(chatMapData(map, [running], [], 'chat').nodes.map(node => node.id)).toEqual(['a', 'b', 'c', 'd']);
});
