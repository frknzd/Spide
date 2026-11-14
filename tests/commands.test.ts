import assert from "node:assert/strict";
import test from "node:test";
import { COMMANDS, normalizeCommand } from "../src/state/command-catalog.ts";

test("palette commands validate required arguments and accept native /spide prefixes", () => {
  assert.equal(normalizeCommand("find").error, "Find class needs a value.");
  assert.deepEqual(normalizeCommand("/spide find Loan"), { command: "find Loan" });
  assert.deepEqual(normalizeCommand("/tidy"), { command: "tidy" });
  assert.equal(normalizeCommand("   ").error, "Enter a command.");
  assert.match(normalizeCommand("add").error!, /class name/);
});

test("every palette entry is unique and documented", () => {
  assert.equal(new Set(COMMANDS.map((c) => c.command)).size, COMMANDS.length);
  for (const c of COMMANDS) assert.ok(c.label && c.help && c.group, c.command);
  for (const c of COMMANDS.filter((x) => x.required)) assert.ok(c.template?.endsWith(" "), c.command);
});
