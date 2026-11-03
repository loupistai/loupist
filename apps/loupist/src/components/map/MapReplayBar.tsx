import { useEffect, useRef } from "react";
import { PauseIcon, PlayIcon, XIcon } from "lucide-react";
import { useTranslation } from "../../i18n";
import { formatDate } from "../../i18n/format";
import { Button } from "../ui/button";
import { DAY_MS, type MapReplay } from "./replay";
/** `range` names a replay that starts at a chosen date, such as "Oct 26 → now". */
export function MapReplayBar({ replay, disabled, range }: { replay: MapReplay; disabled?: boolean; range?: string }) {
  const { t } = useTranslation("knowledge");
  const playButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (!disabled) playButton.current?.focus(); }, []);
  const date = formatDate(replay.time);
  return <div className="map-replay-bar" role="group" aria-label={t("map.replay.label")} onKeyDown={event => {
    if (disabled) return;
    if (event.key === " ") { event.preventDefault(); replay.toggle(); }
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); replay.seek(replay.time + (event.key === "ArrowLeft" ? -DAY_MS : DAY_MS)); }
  }}>
    <Button ref={playButton} variant="ghost" size="icon" disabled={disabled} data-map-play aria-label={t(replay.playing ? "map.replay.pause" : "map.replay.play")} onClick={replay.toggle}>{replay.playing ? <PauseIcon /> : <PlayIcon />}</Button>
    <input className="map-replay-scrubber" type="range" min={replay.start} max={Math.max(replay.start + 1, replay.end)} step={1} value={replay.time} disabled={disabled} aria-label={t("map.replay.timeline")} aria-valuetext={date} style={{ background: `linear-gradient(to right, var(--gg-color-accent) ${100 * (replay.time - replay.start) / Math.max(1, replay.end - replay.start)}%, var(--gg-color-border-default) 0)` }} onPointerDown={() => replay.seek(replay.time)} onChange={event => replay.seek(Number(event.target.value))} />
    {range ? <span className="map-replay-range">{range}</span> : null}
    <span className="map-replay-date gg-num">{date}</span>
    <div className="map-replay-speeds">{[1, 4, 16].map(speed => <Button key={speed} variant="ghost" disabled={disabled} aria-pressed={replay.speed === speed} onClick={() => replay.setSpeed(speed)}>{speed}×</Button>)}</div>
    <Button variant="ghost" size="icon" disabled={disabled} aria-label={t("map.replay.close")} onClick={replay.close}><XIcon /></Button>
  </div>;
}
