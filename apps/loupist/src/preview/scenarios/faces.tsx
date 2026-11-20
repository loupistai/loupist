/** The Loupist's face on its own: every face colour at every size it is drawn, on each surface. */
import { LoupistAvatar } from "../../components/app/LoupistAvatar";
import { LOUPIST_PORTRAIT_IDS } from "../../contracts/loupist";
import { prepareFixture, previewKo } from "../fixtures";
import { select, type PreviewScenario } from "./kit";

// Every size the app draws it at: the switcher list, the default, Bot and the switcher, the header, the greeting, onboarding.
const SIZES = [24, 28, 32, 36, 56, 64] as const;
// Preview fixture headings in both languages.
const SURFACES = [
  { id: "page", label: ["페이지", "Page"] },
  { id: "sidebar", label: ["사이드바", "Sidebar"] },
  { id: "raised", label: ["카드", "Card"] },
] as const;
const BUSY = ["작업 중", "Working"] as const;

// Gallery layout only; the face carries its own styles (styles/loupist-face.css).
const GALLERY_CSS = `
.preview-faces { box-sizing: border-box; height: 100vh; overflow: auto; padding: var(--gg-space-6); display: grid; gap: var(--gg-space-4); align-content: start; background: var(--gg-color-bg-app); color: var(--gg-color-text-primary); font-family: var(--gg-font-ui); }
.preview-faces h2 { margin: 0 0 var(--gg-space-2); font-size: var(--gg-text-meta); font-weight: var(--gg-weight-semibold); color: var(--gg-color-text-tertiary); }
.preview-faces-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: var(--gg-space-3); padding: var(--gg-space-4); border-radius: var(--gg-radius-lg); border: 1px solid var(--gg-color-border-subtle); }
.preview-faces-grid[data-surface="page"] { background: var(--gg-color-bg-app); }
.preview-faces-grid[data-surface="sidebar"] { background: var(--gg-color-bg-sidebar); }
.preview-faces-grid[data-surface="raised"] { background: var(--gg-color-bg-raised); box-shadow: var(--gg-shadow-card); }
.preview-faces-row { display: flex; align-items: center; gap: var(--gg-space-3); }
`;

function FaceGallery() {
  const ko = previewKo();
  return <main className="preview-faces" data-face-gallery>
    <style>{GALLERY_CSS}</style>
    {SURFACES.map(surface => <section key={surface.id}>
      <h2>{surface.label[ko ? 0 : 1]}</h2>
      <div className="preview-faces-grid" data-surface={surface.id}>
        {LOUPIST_PORTRAIT_IDS.map(avatar => <div key={avatar} className="preview-faces-row">
          {SIZES.map(size => <LoupistAvatar key={size} loupist={{ id: avatar, name: avatar, avatar }} size={size} />)}
        </div>)}
      </div>
    </section>)}
    <section>
      <h2>{BUSY[ko ? 0 : 1]}</h2>
      <div className="preview-faces-grid" data-surface="raised">
        <div className="preview-faces-row">{SIZES.map(size => <LoupistAvatar key={size} loupist={{ id: "busy", name: "busy", avatar: "loupist-01" }} size={size} busy />)}</div>
      </div>
    </section>
  </main>;
}

export const faceScenarios: PreviewScenario[] = [
  {
    id: "loupist-faces",
    title: "The Loupist’s face: ten colours at 24, 28, 32, 36, 56 and 64 px on page, sidebar and card",
    setup: (bridge) => prepareFixture(bridge),
    enter: async () => {},
    landmark: () => document.querySelector("[data-face-gallery] .loupist-face") ? select("[data-face-gallery] .preview-faces-grid") : null,
    render: () => <FaceGallery />,
  },
];
