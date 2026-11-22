import { ServerResponse } from "node:http";

// The distributed 128px macOS app icon, embedded unchanged inside the inline SVG.
const mark = '<svg width="56" height="56" viewBox="0 0 128 128" aria-hidden="true" xmlns="http://www.w3.org/2000/svg"><image width="128" height="128" href="data:image/png;base64,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"/></svg>';
const entities = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const escape = text => text.replace(/[&<>"']/gu, ch => entities[ch]);
const unescape = text => text.replace(/&(amp|lt|gt|quot|#39);/gu, entity =>
  ({ "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'" })[entity]);

export function signInPage({ success, language, provider, reason = "" }) {
  const ko = language === "ko";
  const heading = success ? (ko ? "Loupist로 돌아가십시오" : "Return to Loupist")
    : (ko ? "로그인을 완료하지 못했습니다" : "Sign-in did not complete");
  const message = success ? (ko ? `${provider}에서 로그인을 승인했습니다. 연결은 앱에서 마무리됩니다.` : `${provider} approved the sign-in. Loupist finishes connecting in the app.`)
    : (ko ? "Loupist로 돌아가 다시 시도하십시오." : "Return to Loupist and try again.");
  const note = success ? (ko ? "이 탭은 닫아도 됩니다." : "You can close this tab.") : reason;
  return `<!doctype html><html lang="${ko ? "ko" : "en"}"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Loupist</title>
<style>
:root { color-scheme: light dark; --bg: #f5f5f8; --primary: #15151c; --secondary: #4f4f66; --tertiary: #66667a }
@media (prefers-color-scheme: dark) { :root { --bg: #111116; --primary: #f1f1f6; --secondary: #b3b3c2; --tertiary: #8c8c9c } }
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg); color: var(--primary); font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif }
main { width: min(420px, calc(100% - 48px)); text-align: center; overflow-wrap: anywhere }
h1 { margin: 16px 0 8px; font-size: 20px; font-weight: 600 }
p { margin: 0; color: var(--secondary); font-size: 15px; line-height: 1.5 }
.note { margin-top: 12px; color: var(--tertiary); font-size: 13px; white-space: pre-wrap }
</style></head><body><main>${mark}<h1>${escape(heading)}</h1><p>${escape(message)}</p><p class="note">${escape(note)}</p></main></body></html>`;
}

function replacement(body, language) {
  if (typeof body !== "string" && !Buffer.isBuffer(body)) return null;
  const html = body.toString();
  const head = /<head>([\s\S]*?)<\/head>/u.exec(html)?.[1];
  const title = head && /<title>([^<]*)<\/title>/u.exec(head)?.[1];
  if (title !== "Authentication successful" && title !== "Authentication failed") return null;
  const success = title === "Authentication successful";
  const message = /<p>([^<]*)<\/p>/u.exec(html)?.[1];
  if (!success && message === undefined) return null;
  const details = /<div class="details">([^<]*)<\/div>/u.exec(html)?.[1];
  const reason = [message, details].filter(Boolean).map(unescape).join("\n");
  return signInPage({ success, language, provider: "ChatGPT", reason });
}

// Pi's loopback handlers call writeHead(status, headers), then end(html).
// Inspect headers here: Node does not retain writeHead's inline values in getHeader().
export async function withSignInPage(provider, language, login) {
  if (provider !== "openai" && provider !== "openai-codex") return login();
  const prototype = ServerResponse.prototype;
  const writeHead = prototype.writeHead;
  let active = true;
  prototype.writeHead = function (...args) {
    const headers = args.at(-1);
    if (!headers || typeof headers !== "object" || Array.isArray(headers)) return writeHead.apply(this, args);
    const keys = Object.keys(headers);
    const type = keys.find(key => key.toLowerCase() === "content-type");
    const mime = type && headers[type];
    if (typeof mime !== "string" || mime.split(";")[0].trim().toLowerCase() !== "text/html") {
      return writeHead.apply(this, args);
    }
    const length = keys.find(key => key.toLowerCase() === "content-length");
    const defer = length !== undefined || this.hasHeader("content-length");
    const { end, write, flushHeaders } = this;
    const finishHead = page => {
      this.end = end;
      this.write = write;
      this.flushHeaders = flushHeaders;
      if (defer) {
        if (page !== null) {
          const bytes = Buffer.byteLength(page);
          if (length === undefined) this.setHeader("content-length", bytes);
          else args[args.length - 1] = { ...headers, [length]: bytes };
        }
        writeHead.apply(this, args);
      }
    };
    this.end = function (body, ...rest) {
      const page = active ? replacement(body, language) : null;
      finishHead(page);
      return end.call(this, page === null ? body : Buffer.from(page, "utf8"), ...rest);
    };
    // A streaming or explicitly flushed response keeps the original headers and body.
    this.write = function (...data) { finishHead(null); return write.apply(this, data); };
    this.flushHeaders = function () { finishHead(null); return flushHeaders.call(this); };
    return defer ? this : writeHead.apply(this, args);
  };
  try {
    return await login();
  } finally {
    active = false;
    prototype.writeHead = writeHead;
  }
}
