import { useEffect, useRef, useState, type MutableRefObject } from "react";
import type { LoupistBridge } from "../../types";
import { useTranslation } from "../../i18n";
import { formatDate, formatNumber } from "../../i18n/format";
import { loupistAvatarSource } from "../../lib/loupistAvatar";
import { Button } from "../ui/button";
import type { MapCanvasControls } from "./MapCanvas";
import { composeMapExport, pngBytes, recordingFormat, type MapExportFooter } from "./export";
import { REPLAY_DURATION_MS, replayTime, type MapReplay } from "./replay";
import { titledExport } from "../../lib/exportTitle";
const nextFrame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
/** Map export as an image or a replay video; the actions live in the toolbar's More menu and the progress and error sit over the Map. */
export function useMapExport({ bridge, loupistId, controls, replay, stats, onRecording }: { bridge: LoupistBridge; loupistId: string; controls: MutableRefObject<MapCanvasControls | null>; replay: MapReplay; stats: string; onRecording: (recording: boolean) => void }) {
  const { t } = useTranslation("knowledge");
  const [progress, setProgress] = useState<number | null>(null), [error, setError] = useState(false);
  const cancel = useRef(false), mounted = useRef(true), busy = useRef(false);
  const current = useRef({ stats, replay }); current.current = { stats, replay };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; cancel.current = true; }; }, []);
  const save = async (video: boolean) => {
    if (busy.current) return;
    busy.current = true; cancel.current = false; setError(false);
    let stream: MediaStream | undefined, recorder: MediaRecorder | undefined;
    const previous = { active: replay.active, playing: replay.playing, time: replay.time, end: replay.end, speed: replay.speed };
    try {
      controls.current?.exportLabels?.(true);
      await nextFrame(); await nextFrame();
      const source = controls.current?.canvas(); if (!source) throw new Error("Canvas unavailable");
      const loupist = (await bridge.loupistList()).find(loupist => loupist.id === loupistId); if (!loupist) throw new Error("Loupist unavailable");
      const footer: MapExportFooter = { name: loupist.name, stats, date: formatDate(replay.active ? replay.time : Date.now()), disclosure: t("map.export.disclosure") };
      const avatar = new Image(); avatar.src = loupistAvatarSource(loupist); await avatar.decode(); footer.avatar = avatar;
      if (cancel.current || !mounted.current) return;
      const name = `${loupist.name.replace(/[/\\:]/g, "-")}-map`;
      if (!video) {
        await bridge.exportSave(titledExport({ suggestedName: `${name}.png`, kind: "png" as const, bytes: Array.from(pngBytes(composeMapExport(source, footer))) }));
        return;
      }
      const { mime, kind } = recordingFormat();
      if (!source.captureStream) throw new Error("Recording unavailable");
      onRecording(true); setProgress(0);
      const end = Date.now();
      replay.setState({ active: true, playing: false, time: replay.start, end, speed: 1 });
      await nextFrame(); await nextFrame();
      const output = composeMapExport(source, footer, true);
      stream = output.captureStream(30); recorder = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 8_000_000 });
      const chunks: Blob[] = []; let size = 0; let encodingError = false;
      const finished = new Promise<Blob>((resolve, reject) => {
        recorder!.ondataavailable = event => { size += event.data.size; if (size > 200 * 1024 * 1024) { encodingError = true; cancel.current = true; reject(new Error("Export too large")); } else chunks.push(event.data); };
        recorder!.onerror = () => { encodingError = true; cancel.current = true; reject(new Error("Recording failed")); };
        recorder!.onstop = () => resolve(new Blob(chunks, { type: mime }));
      });
      // Attach rejection handling immediately; encoder errors may arrive mid-replay.
      void finished.catch(() => {});
      recorder.start(1000);
      const began = performance.now();
      while (!cancel.current) {
        const fraction = Math.min(1, (performance.now() - began) / REPLAY_DURATION_MS);
        const time = replayTime(replay.start, end, fraction, replay.reduced);
        replay.setState(s => ({ ...s, time }));
        await nextFrame(); await nextFrame();
        composeMapExport(source, { ...footer, date: formatDate(time), stats: current.current.stats }, true, output);
        if (mounted.current) setProgress(Math.round(fraction * 100));
        if (fraction >= 1) break;
      }
      if (recorder.state !== "inactive") recorder.stop();
      if (encodingError) cancel.current = false;
      const blob = await finished;
      if (!cancel.current) await bridge.exportSave(titledExport({ suggestedName: `${name}.${kind}`, kind, bytes: Array.from(new Uint8Array(await blob.arrayBuffer())) }));
    } catch { if (mounted.current && !cancel.current) setError(true); }
    finally {
      controls.current?.exportLabels?.(false);
      if (recorder && recorder.state !== "inactive") recorder.stop();
      stream?.getTracks().forEach(track => track.stop());
      if (video && mounted.current) replay.setState(previous);
      if (mounted.current) { setProgress(null); onRecording(false); }
      busy.current = false;
    }
  };
  const status = <>
    {progress !== null ? <div className="map-recording" role="status"><span>{t("map.export.recording", { pct: formatNumber(progress) })}</span><Button variant="outline" onClick={() => { cancel.current = true; }}>{t("map.export.cancel")}</Button></div> : null}
    {error ? <div className="map-export-error" role="alert">{t("map.export.error")}<Button variant="ghost" onClick={() => setError(false)}>{t("map.export.dismiss")}</Button></div> : null}
  </>;
  return { image: () => void save(false), video: () => void save(true), busy: progress !== null, status };
}
