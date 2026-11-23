import assert from "node:assert/strict";
import { waitForNoLayers, waitForNoRuns } from "./native-shell.mjs";

// A 2×2 PNG: small enough to inline, a real raster for WebKit to decode.
const PNG_BASE64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGM4YaMBRAwQCgAjXgSxbgN+HAAAAABJRU5ErkJggg==";

/**
 * Attaches an image and a text file through the composer's file input (what the + menu's Files…
 * chooser fills; WebDriver on macOS cannot drive the OS chooser), sends them, and checks that the tray
 * previewed the image and that the sent message shows both. Under the release renderer the shipped
 * Content-Security-Policy is enforced, which once blocked the blob: preview and the blob: read on send.
 */
export async function verifyComposerAttachments(browser, { message = "Read the attached chart and notes.", timeout = 20_000 } = {}) {
  const attached = await browser.execute((png) => {
    const input = document.querySelector('input[type="file"][aria-label="Upload files"]');
    if (!input) return "no file input";
    const transfer = new DataTransfer();
    transfer.items.add(new File([Uint8Array.from(atob(png), (character) => character.charCodeAt(0))], "chart.png", { type: "image/png" }));
    transfer.items.add(new File(["quarter,revenue\nQ1,120\nQ2,135\n"], "notes.csv", { type: "text/csv" }));
    input.files = transfer.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    return null;
  }, PNG_BASE64);
  assert.equal(attached, null, attached ?? "");

  const tray = () => browser.execute(() => {
    const form = document.querySelector("form:has(textarea[data-chat-composer])");
    const image = form?.querySelector('img[alt="chart.png"]');
    return {
      image: image ? { decoded: image.complete && image.naturalWidth > 0, scheme: image.getAttribute("src")?.split(":")[0] } : null,
      file: Boolean(form && [...form.querySelectorAll("span")].some((span) => span.textContent === "notes.csv")),
    };
  });
  let previewed;
  await browser.waitUntil(async () => {
    previewed = await tray();
    return previewed.image?.decoded && previewed.file;
  }, { timeout, interval: 100, timeoutMsg: "The composer did not preview the attached image and list the file" });
  assert.equal(previewed.image.scheme, "data", "The tray previews the image from its bytes (data:), which the policy allows");

  const composer = await browser.$("textarea[data-chat-composer]");
  await composer.setValue(message);
  await (await browser.$('form:has(textarea[data-chat-composer]) button[aria-label="Send"]')).click();

  let sent;
  await browser.waitUntil(async () => {
    sent = await browser.execute((text) => {
      const turn = [...document.querySelectorAll("#main-panel-chat [data-message-id]")].reverse()
        .find((element) => element.textContent?.includes(text));
      if (!turn) return { found: false, error: document.querySelector(".composer-error")?.textContent ?? null };
      const image = turn.querySelector('button[aria-label="Open chart.png"] img');
      return {
        found: true,
        image: Boolean(image && image.complete && image.naturalWidth > 0),
        file: Boolean(turn.querySelector('button[aria-label="Download notes.csv"]')),
      };
    }, message);
    return sent.found && sent.image && sent.file;
  }, { timeout, interval: 200 }).catch((error) => {
    throw new Error(`The sent message did not show both attachments: ${JSON.stringify(sent)}`, { cause: error });
  });
  await settleReply(browser);
  return { previewed, sent };
}

/**
 * Leaves nothing behind: the reply to the sent message is stopped (the fixture model has nothing to
 * say outside a hand-off), the host runs nothing, no menu or dialog is open and the composer offers Send
 * again. A reply still running on a slow runner once made the next step's Remove Loupist refuse and keep
 * its dialog over the header.
 */
async function settleReply(browser) {
  const stop = '#main-panel-chat button[aria-label="Stop response"]';
  if (await browser.$(stop).isDisplayed().catch(() => false)) await browser.$(stop).click().catch(() => {});
  await browser.waitUntil(async () => !(await browser.$(stop).isDisplayed().catch(() => false)), {
    timeout: 30_000, interval: 200, timeoutMsg: "The reply to the attachments did not stop",
  });
  await waitForNoRuns(browser);
  await waitForNoLayers(browser);
  await browser.$('form:has(textarea[data-chat-composer]) button[aria-label="Send"]').waitForDisplayed({
    timeout: 10_000, timeoutMsg: "The composer did not return to Send",
  });
}
