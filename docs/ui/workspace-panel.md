# Workspace panel

The header's right controls are **Panel kind | Split top and bottom | Split left and right**. Panel kind has three canonical entries, in this order: **Map** (지도), **Lens** (렌즈), and **Chat materials** (대화 자료). The word World never appears in UI chrome. Lens opens the Loupist's research principles and methods; Map opens its observed map, Library, replay and export; Chat materials holds the existing chat map, Evidence, Work, browser and workspace-file readers. Each menu entry carries a one-line description under its name (Lens "Your research principles and methods", Map "Map of companies, people and links", Chat materials "This chat's map, evidence and outputs"), so a first-time user can tell them apart. Selecting an entry opens it beside the current chat without changing chats or the composer's draft, files, pins, as-of date or selected artifact. Explicit answer links, citations, artifacts and workspace files select Chat materials; a background document badges its Work tab while Lens or Map is being read. A Lens page opened from chat (a `lens:` link, a learned page's title, a saved Lens change) always opens in the Evidence reader; only that reader's Open in Lens switches the panel kind. A principle has no page to read there, so its View opens on What {Loupist} knows. A tab opened from chat leaves focus where it was, and one polite live region in the app shell says "Opened {title} in the panel" in the interface language, a Lens record's title included.

A new profile starts with the panel closed and Chat materials selected. A saved layout restores through Rust's bounded workspace UI state. A header split choice applies to all content kinds and is stored there with the panel dimensions; pressing the active split closes the panel, and choosing a kind opens it again. Narrow windows retain the existing focus-trapped sheet instead of forcing a split that cannot fit. Legacy Map/Lens navigation, command-palette actions, ⌘2/⌘3 and knowledge deep links resolve to Map/Lens in this same panel. The former saved Map/Lens last-view preference migrates to the panel once. Panel-kind selection itself is session state, while the retained readers stay per-chat bounded typed references.

The panel lists charts, tables and documents for the open Loupist and restores only bounded references. It does not open itself for an empty state or by moving keyboard focus. The first names a chat saves do open the Map tab beside that chat when the window is at least 1280px wide and the panel is not the narrow sheet. If the user closed the panel in that chat, it stays closed. If they are already reading another tab, the Map tab is marked instead of taking over.

Readers preserve selection, focus and scroll through navigation. Workspace panel
preferences store bounded references, not reader bodies; restoration repeats
ownership checks. A stored file that is too large, malformed, of an older shape or
fails validation reads as absent, so the next save replaces it; I/O failures still
surface as errors and never overwrite a good layout. The panel has one Work tab that
lists the Loupist's charts, tables and documents across its chats (Rust `loupist_artifact_list`,
newest first, with chat titles), with "Preparing" rows for charts still streaming;
an item opens inside the tab under a status row with its title, a "Version n ▾" menu of the revisions Rust keeps (`artifact_versions`, newest first with dates) and the shown revision's date, and reads from its own chat. Choosing an earlier version reads it by `revision_id` read-only; "Restore this version" asks once, then `artifact_restore` appends it as the newest revision and the panel follows it, while answers keep the revisions they showed. Charts dock at the bottom
and reading content at the side by default; using Dock records the choice per kind
(`dock_by_kind`), and the dock changes only when a different kind comes on screen. "Always dock here" in the panel menu stores the current placement for every kind (checked while every kind docks there); turning it off clears the choices back to the defaults. A closed panel stays closed when background work finishes; document arrivals are retained in Work, while charts and tables render inline. Closing it marks that chat `dismissed` in the Rust-validated workspace UI state until the user opens it again. Content that arrives while
the user is working in the panel marks its tab with a dot instead of switching. The header
kind selector and split buttons stay visible while the panel is open. Below 1060px the sheet is positioned in the stage under the titlebar, so those header controls are not covered. When the stage is at least 840px wide a side sheet leaves a strip of the chat, and the chat wraps into that strip so a line is not sliced by the sheet. Under 840px of stage the sheet covers the whole stage, because a strip that narrow cannot be read. Close on the panel header, pressing the active split, ⌘\, Escape, and the narrow sheet's backdrop close it. A keyboard kind selection or split activation moves focus to Close. Tabs and Add stay on the tab
bar, and the Add menu stays above the browser address field. The tab strip is light underline tabs on the panel background: the selected tab shows a 2px underline under its content and no fill, a hovered tab gets a soft rounded background, and one hairline separates the fixed Map, Evidence and Work tabs from reader tabs only when reader tabs exist. Lens and World show a heading row of the same 42px height and text inset, so changing kind does not move the content. A page tab keeps its title, up to 220px, instead of shrinking until a short host is cut; the tab row scrolls the selected tab into view when Map, Evidence and Work take the rest of the row. Expand, Dock and "Always dock here" live in one panel menu on the docked panel; the
narrow overlay offers Close only. Expand on the Map tab opens World
on the name selected in the mini map; on any other tab it covers the main area beside
the sidebar with that tab in one reading column and "Back to chat" (Esc returns first,
the next Esc closes the panel; the chat underneath is inert). Closing the panel
returns focus to the Panel kind selector currently shown on the chat when the control that
opened it is no longer in that chat. Leaving the
reading column returns focus to the panel menu.

A long page title truncates inside its 220px tab, before Close. In a wide Lens,
the rule/method/history list and its document stay in separate, independently
scrolling columns; a long list cannot take away the document's height. In a compact
Lens, opening an item replaces the list, and Back restores it. Compact section tabs
retain their full accessible names when the visible labels are shortened.
