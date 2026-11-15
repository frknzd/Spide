-- Plugin unit tests in an isolated Neovim:
--   nvim --headless --clean -l integrations/nvim/tests/run.lua
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)")
vim.opt.rtp:prepend(here .. "..")
local spide = require("spide")

local failures = 0
local function test(name, f)
  local ok, e = pcall(f)
  print((ok and "ok   " or "FAIL ") .. name .. (ok and "" or (": " .. tostring(e))))
  if not ok then
    failures = failures + 1
  end
end

local dir = vim.fn.tempname()
vim.fn.mkdir(dir .. "/.spide", "p")
local file = dir .. "/a.py"
vim.fn.writefile({ "one", "two", "three" }, file)

test("buffer_text matches the file bytes (eol)", function()
  vim.cmd.edit(file)
  local t = spide.buffer_text(file)
  assert(t.text == "one\ntwo\nthree\n", vim.inspect(t.text))
  assert(t.dirty == false)
end)

test("apply_edits is guarded by changedtick and joins undo", function()
  local st = spide.buffer_state(file)
  local bad = spide.apply_edits(file, st.changedtick - 1, { { start_line = 1, end_line = 1, text = "X" } })
  assert(not bad.ok and bad.error:find("changed"))
  local ok = spide.apply_edits(file, st.changedtick, { { start_line = 1, end_line = 1, text = "ONE" }, { start_line = 3, end_line = 2, text = "a\nb" } })
  assert(ok.ok, vim.inspect(ok))
  assert(vim.deep_equal(vim.api.nvim_buf_get_lines(0, 0, -1, false), { "ONE", "two", "a", "b", "three" }))
  vim.cmd("silent undo")
  assert(vim.deep_equal(vim.api.nvim_buf_get_lines(0, 0, -1, false), { "one", "two", "three" }), "one undo step")
end)

test("dirty buffers and save_all", function()
  vim.api.nvim_buf_set_lines(0, 0, 1, false, { "changed" })
  assert(#spide.dirty_buffers() == 1)
  assert(#spide.save_all() == 0)
  assert(vim.fn.readfile(file)[1] == "changed")
end)

test("open moves the cursor to line/byte column", function()
  local r = spide.open(file, 2, 1)
  assert(r.ok)
  local c = vim.api.nvim_win_get_cursor(0)
  assert(c[1] == 2 and c[2] == 1, vim.inspect(c))
end)

test("discover ignores dead endpoints", function()
  vim.fn.writefile({ vim.json.encode({ root = dir, pid = 999999, mcp_url = "http://127.0.0.1:1/mcp", token = "x", host = "headless" }) }, dir .. "/.spide/endpoint.json")
  vim.env.XDG_STATE_HOME = dir .. "/state"
  assert(spide.discover(dir) == nil)
  vim.fn.writefile({ vim.json.encode({ root = dir, pid = vim.fn.getpid(), mcp_url = "http://127.0.0.1:1/mcp", token = "x", host = "headless" }) }, dir .. "/.spide/endpoint.json")
  assert(spide.discover(dir).token == "x")
end)

test("call reports an unreachable Spide instead of hanging", function()
  local t = vim.uv.hrtime()
  local r, e = spide.call("get_design", {}, { timeout = 1000 })
  assert(r == nil and e, "expected an error")
  assert((vim.uv.hrtime() - t) / 1e9 < 3)
end)

vim.fn.delete(dir, "rf")
os.exit(failures == 0 and 0 or 1)
