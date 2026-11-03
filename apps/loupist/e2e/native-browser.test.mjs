import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { browserProbeEnvelope } from "./native-browser.mjs";

// webdriver isSuccessfulResponse rejects a truthy top-level error, stackTrace,
// or stacktrace on the execute/sync value. Nesting avoids that.
function clientWouldReject(value) {
  const body = { value };
  return Boolean(body.value && (
    (typeof body.value === "object" && "error" in body.value && body.value.error)
    || (typeof body.value === "object" && "stackTrace" in body.value && body.value.stackTrace)
    || (typeof body.value === "object" && "stacktrace" in body.value && body.value.stacktrace)
  ));
}

function envelopeFor(probe) {
  return runInNewContext(`(${browserProbeEnvelope})()`, {
    window: { __nativeBrowserProbe: probe },
  });
}

test("a denied probe stays nested so WebDriver can return it", () => {
  const denial = {
    pending: false,
    result: null,
    error: {
      code: "browser_url_unsupported_protocol",
      message: "only http and https links can be opened",
    },
    events: [{ type: "load_finished", tab_id: "tab", url: "http://127.0.0.1/article" }],
  };
  assert.equal(clientWouldReject(denial), true);
  const envelope = envelopeFor(denial);
  assert.equal(clientWouldReject(envelope), false);
  assert.equal(JSON.stringify(envelope.probe), JSON.stringify(denial));
  assert.equal(envelope.probe.error.code, denial.error.code);
});

test("an empty probe read is not a protocol error", () => {
  const envelope = envelopeFor(undefined);
  assert.equal(envelope, null);
  assert.equal(clientWouldReject(envelope), false);
});
