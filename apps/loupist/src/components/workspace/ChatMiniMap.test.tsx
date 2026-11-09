import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage, KnowledgeLocation } from "../../types";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { ChatMapProvider, useChatMap } from "./ChatMapContext";
import { ChatMiniMap } from "./ChatMiniMap";
import { MessageMemoryRow } from "../chat/MessageMemoryRow";
import { WorkspaceTabBar } from "./WorkspaceTabBar";
vi.mock('../map/MapCanvas', () => ({ default: ({ compact, stats }: { compact: { highlightIds: Set<string>; glowNodeIds: Set<string>; glowEdgeIds: Set<string>; liveNodeIds: Set<string> }; stats: string }) => <div role="img" aria-label={stats} data-highlight={[...compact.highlightIds].join(',')} data-glow={[...compact.glowNodeIds, ...compact.glowEdgeIds].join(',')} data-live={[...compact.liveNodeIds].join(',')} /> }));
const answer: ChatMessage = { id: 'answer', role: 'assistant', content: 'Answer', created_at: '2026-10-28', status: 'complete' };
const map = { nodes: ['a', 'b', 'c'].map(id => ({ id, label: id, kind: 'entity', degree: 1, watched: false, held: false })), edges: [{ id: 'ab', kind: 'claim', source: 'a', target: 'b' }], worldRevisionId: 'rev' } as KnowledgeMap;
const event = (id: string, chatId = 'chat', messageId = 'answer'): GrowthEvent => ({ eventId: `${chatId}:${id}`, kind: 'entity_added', loupistId: 'loupist', subjectId: id, atMs: Date.now(), originChatId: chatId, originMessageId: messageId, recordId: 'world', actor: 'loupist', summary: '', beforeSummary: null, afterSummary: null });
function setup(events: GrowthEvent[] = [event('a'), event('b'), event('c')]) {
 const bridge = new MockLoupistBridge({ delay_ms: 0 });
 vi.spyOn(bridge, 'knowledgeMap').mockResolvedValue(map);
 vi.spyOn(bridge, 'knowledgeGrowthEvents').mockResolvedValue({ events, nextCursor: null });
 return bridge;
}
function Surface({ onOpenFull = vi.fn() }: { onOpenFull?: (location: KnowledgeLocation) => void }) {
 const state = useChatMap();
 return <><MessageMemoryRow loupistId="loupist" chatId={state?.chatId} message={answer} /><WorkspaceTabBar session={{ tabs: [] }} menuOpen={false} onMenuOpenChange={vi.fn()} onSelectTab={vi.fn()} onCloseTab={vi.fn()} onNewBrowser={vi.fn()} />{state?.active ? <ChatMiniMap onOpenFull={onOpenFull} /> : null}</>;
}
it('opens the local Map from its answer chip, highlights every added node, and offers list and full-map actions', async () => {
 const bridge = setup(); const onOpen = vi.fn(), onOpenFull = vi.fn();
 render(<ChatMapProvider bridge={bridge} loupistId="loupist" chatId="chat" messages={[answer]} onOpen={onOpen}><Surface onOpenFull={onOpenFull} /></ChatMapProvider>);
 fireEvent.click(await screen.findByRole('button', { name: /Show 3 new on the map/ }));
 expect(onOpen).toHaveBeenCalledOnce();
 expect(screen.getByRole('tab', { name: 'Map' })).toHaveAttribute('aria-selected', 'true');
 expect(await screen.findByRole('img')).toHaveAttribute('data-highlight', 'a,b,c');
 fireEvent.click(screen.getByRole('button', { name: 'List view' }));
 expect(screen.getByRole('table', { name: 'List view' })).toBeVisible();
 fireEvent.click(screen.getByRole('button', { name: 'Open full map →' }));
 expect(onOpenFull).toHaveBeenCalledWith({ loupistId: 'loupist', surface: 'world', selection: undefined });
});
it('keeps other chats and answers out of the chip and resets immediately on a chat switch', async () => {
 const bridge = setup([event('a', 'other'), event('b', 'chat', 'earlier')]);
 const props = { bridge, loupistId: 'loupist', chatId: 'chat', messages: [answer], onOpen: vi.fn() };
 const view = render(<ChatMapProvider {...props}><Surface /></ChatMapProvider>);
 await waitFor(() => expect(bridge.knowledgeGrowthEvents).toHaveBeenCalled());
 expect(screen.queryByRole('button', { name: /on the map/ })).toBeNull();
 fireEvent.click(screen.getByRole('tab', { name: 'Map' }));
 await screen.findByRole('img');
 view.rerender(<ChatMapProvider {...props} chatId="empty"><Surface /></ChatMapProvider>);
 expect(screen.queryByRole('img')).toBeNull();
 fireEvent.click(screen.getByRole('tab', { name: 'Map' }));
 expect(await screen.findByText('Companies and topics from this chat appear here.')).toBeVisible();
});
it('retries failed requests and reads every growth-event page before filtering the chat', async () => {
 const bridge = setup();
 vi.mocked(bridge.knowledgeMap).mockRejectedValueOnce(new Error('offline'));
 vi.mocked(bridge.knowledgeGrowthEvents).mockResolvedValueOnce({ events: [event('a', 'other')], nextCursor: 'next' }).mockResolvedValue({ events: [event('b')], nextCursor: null });
 render(<ChatMapProvider bridge={bridge} loupistId="loupist" chatId="chat" messages={[answer]} onOpen={vi.fn()}><Surface /></ChatMapProvider>);
 fireEvent.click(screen.getByRole('tab', { name: 'Map' }));
 expect(await screen.findByRole('alert')).toBeVisible();
 fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
 expect(await screen.findByRole('button', { name: /Show 1 new on the map/ })).toBeVisible();
 expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledWith({ loupistId: 'loupist', limit: 5000, cursor: 'next' });
});
it('fetches on a completed turn without polling or refetching for streaming text', async () => {
 vi.useFakeTimers();
 try {
 const bridge = setup([]);
 const base = { bridge, loupistId: 'loupist', chatId: 'chat', onOpen: vi.fn() };
 const view = render(<ChatMapProvider {...base} messages={[answer]}><Surface /></ChatMapProvider>);
 await act(async () => { await Promise.resolve(); await Promise.resolve(); });
 expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(1);
 const streaming: ChatMessage = { ...answer, id: 'next', status: 'streaming', content: 'partial' };
 view.rerender(<ChatMapProvider {...base} messages={[answer, streaming]}><Surface /></ChatMapProvider>);
 await act(async () => { vi.advanceTimersByTime(30000); });
 expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(1);
 view.rerender(<ChatMapProvider {...base} messages={[answer, { ...streaming, content: 'finished', status: 'complete' }]}><Surface /></ChatMapProvider>);
 await act(async () => { await Promise.resolve(); await Promise.resolve(); });
 expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(2);
 } finally { vi.useRealTimers(); }
});
it('shows the answer chip once World processing delivers the saved Source after the turn', async () => {
 vi.useFakeTimers({ shouldAdvanceTime: true });
 try {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, 'knowledgeMap').mockResolvedValue(map);
  const growth = vi.spyOn(bridge, 'knowledgeGrowthEvents').mockResolvedValueOnce({ events: [{ ...event('source'), kind: 'source_retained' } as GrowthEvent], nextCursor: null }).mockResolvedValue({ events: [event('a')], nextCursor: null });
  const saved: ChatMessage = { ...answer, memory_update: { status: 'applied', commitId: 'commit', changes: [{ kind: 'Source', title: 'Filing', recordId: 'source-1', operation: 'create' }] } as ChatMessage['memory_update'] };
  function SavedSurface() { const state = useChatMap(); return <MessageMemoryRow loupistId="loupist" chatId={state?.chatId} message={saved} />; }
  render(<ChatMapProvider bridge={bridge} loupistId="loupist" chatId="chat" messages={[saved]} onOpen={vi.fn()}><SavedSurface /></ChatMapProvider>);
  await waitFor(() => expect(growth).toHaveBeenCalledTimes(1));
  expect(screen.queryByRole('button', { name: /Show 1 new on the map/ })).toBeNull();
  await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
  expect(await screen.findByRole('button', { name: /Show 1 new on the map/ })).toBeVisible();
 } finally { vi.useRealTimers(); }
});
const streamingRead = (entityIds: string[]): ChatMessage => ({ id: 'live', role: 'assistant', content: '', created_at: '2026-10-28', status: 'streaming', progress: { startedAtMs: 1, items: [{ id: 'read', kind: 'tool', category: 'memory', operation: 'read', action: 'Read the map', status: 'running', entityIds }] } });
function MapOnly() {
 const state = useChatMap();
 return state?.active ? <ChatMiniMap onOpenFull={vi.fn()} /> : <button type="button" onClick={() => state?.select()}>Show map</button>;
}
function RunningReply({ chatId = 'chat', ids }: { chatId?: string; ids: string[] }) {
 const report = useChatMap()?.reportLive;
 const key = ids.join(',');
 useEffect(() => { report?.('loupist', chatId, key ? key.split(',') : []); }, [report, chatId, key]);
 return null;
}
it('lights the names a running step reads only while the reply runs, and only for its own chat', async () => {
 const bridge = setup([]);
 const props = { bridge, loupistId: 'loupist', chatId: 'chat', onOpen: vi.fn(), messages: [] };
 const view = render(<ChatMapProvider {...props}><RunningReply ids={['a', 'c', 'gone']} /><MapOnly /></ChatMapProvider>);
 fireEvent.click(await screen.findByRole('button', { name: 'Show map' }));
 await waitFor(() => expect(screen.getByRole('img')).toHaveAttribute('data-live', 'a,c'));
 expect(screen.getByRole('img')).toHaveAttribute('data-glow', '');
 // The finished reply keeps its step names on this chat's map, without the live glow.
 const finished = { ...props, messages: [{ ...streamingRead(['a', 'c']), status: 'complete' as const }] };
 view.rerender(<ChatMapProvider {...finished}><RunningReply ids={[]} /><MapOnly /></ChatMapProvider>);
 expect(screen.getByRole('img')).toHaveAttribute('data-live', '');
 view.rerender(<ChatMapProvider {...finished}><RunningReply chatId="other" ids={['a']} /><MapOnly /></ChatMapProvider>);
 expect(screen.getByRole('img')).toHaveAttribute('data-live', '');
});
it('marks the Map tab at a chat\'s first names when the panel is busy, and only once', async () => {
 const bridge = setup([]);
 const onFirstNames = vi.fn<() => 'opened' | 'badged' | 'none'>().mockReturnValue('badged');
 const props = { bridge, loupistId: 'loupist', chatId: 'chat', messages: [answer], onOpen: vi.fn(), onFirstNames };
 const view = render(<ChatMapProvider {...props}><Surface /></ChatMapProvider>);
 await waitFor(() => expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(1));
 expect(onFirstNames).not.toHaveBeenCalled();
 vi.mocked(bridge.knowledgeGrowthEvents).mockResolvedValue({ events: [event('a')], nextCursor: null });
 view.rerender(<ChatMapProvider {...props} refreshKey={1}><Surface /></ChatMapProvider>);
 await waitFor(() => expect(onFirstNames).toHaveBeenCalledOnce());
 expect(await screen.findByText('New in Map')).toBeInTheDocument();
 vi.mocked(bridge.knowledgeGrowthEvents).mockResolvedValue({ events: [event('a'), event('b')], nextCursor: null });
 view.rerender(<ChatMapProvider {...props} refreshKey={2}><Surface /></ChatMapProvider>);
 await waitFor(() => expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(3));
 expect(onFirstNames).toHaveBeenCalledOnce();
});
it('shows the Map tab itself when the panel accepts the first names', async () => {
 const bridge = setup([]);
 const onFirstNames = vi.fn<() => 'opened' | 'badged' | 'none'>().mockReturnValue('opened');
 const props = { bridge, loupistId: 'loupist', chatId: 'chat', messages: [answer], onOpen: vi.fn(), onFirstNames };
 const view = render(<ChatMapProvider {...props}><MapOnly /></ChatMapProvider>);
 await waitFor(() => expect(bridge.knowledgeGrowthEvents).toHaveBeenCalledTimes(1));
 vi.mocked(bridge.knowledgeGrowthEvents).mockResolvedValue({ events: [event('a')], nextCursor: null });
 view.rerender(<ChatMapProvider {...props} refreshKey={1}><MapOnly /></ChatMapProvider>);
 expect(await screen.findByRole('img')).toBeInTheDocument();
 expect(props.onOpen).not.toHaveBeenCalled();
});

it('shows a queued source as waiting until its provider is ready, then clears on completion and chat switch', async () => {
 vi.useFakeTimers({ shouldAdvanceTime: true });
 try {
  const bridge = setup([]);
  const jobs = vi.spyOn(bridge, 'knowledgeGraphJob').mockResolvedValue({ jobId: 'job', status: 'queued' } as Awaited<ReturnType<typeof bridge.knowledgeGraphJob>>);
  const settings = vi.spyOn(bridge, 'knowledgeGraphSettingsGet').mockResolvedValue({ readiness: { status: 'provider_disconnected' } } as Awaited<ReturnType<typeof bridge.knowledgeGraphSettingsGet>>);
  const saved: ChatMessage = { ...answer, memory_update: { status: 'applied', commitId: 'commit', changes: [{ kind: 'Source', title: 'Filing', recordId: 'source-1', operation: 'create', worldJobId: 'job', worldStatus: 'queued' }] } as ChatMessage['memory_update'] };
  function Status() { const state = useChatMap(); return <p role="status">{state?.learningWaiting ?? (state?.learningPending ? 'processing' : 'idle')}</p>; }
  const props = { bridge, loupistId: 'loupist', chatId: 'chat', messages: [saved], onOpen: vi.fn() };
  const view = render(<ChatMapProvider {...props}><Status /></ChatMapProvider>);
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('provider_disconnected'));
  settings.mockResolvedValue({ readiness: { status: 'ready' } } as Awaited<ReturnType<typeof bridge.knowledgeGraphSettingsGet>>);
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(screen.getByRole('status')).toHaveTextContent('processing');
  settings.mockRejectedValueOnce(new Error('offline'));
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(screen.getByRole('status')).toHaveTextContent('unknown');
  jobs.mockResolvedValue({ jobId: 'job', status: 'completed' } as Awaited<ReturnType<typeof bridge.knowledgeGraphJob>>);
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(screen.getByRole('status')).toHaveTextContent('idle');
  expect(bridge.knowledgeMap).toHaveBeenCalledTimes(2);
  view.rerender(<ChatMapProvider {...props} chatId="other" messages={[]}><Status /></ChatMapProvider>);
  expect(screen.getByRole('status')).toHaveTextContent('idle');
 } finally { vi.useRealTimers(); }
});
