import { test } from "node:test";
import assert from "node:assert/strict";
import { modifiedKey } from "../src/state/terminal-keys.ts";
const e = { key: "Enter", type: "keydown", shiftKey: true, ctrlKey: false, altKey: false, metaKey: false };
test("modified keys only use negotiated modifyOtherKeys; legacy bindings stay native", () => {
  assert.equal(modifiedKey(e, false), null);
  assert.equal(modifiedKey(e, true), "\x1b[27;2;13~");
  assert.equal(modifiedKey({ ...e, key: "P", ctrlKey: true }, true), "\x1b[27;6;112~");
  assert.equal(modifiedKey({ ...e, key: "p", shiftKey: false, ctrlKey: true }, true), null);
  assert.equal(modifiedKey({ ...e, type: "keyup" }, true), null);
  assert.equal(modifiedKey({ ...e, key: "Tab" }, true), null);
});
