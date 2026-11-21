-- spide.nvim — Neovim as Spide's editor.
--
-- Neovim → Spide: talks to the Spide process owning this project over its authenticated MCP
-- endpoint (discovered from `.spide/endpoint.json` or `$XDG_STATE_HOME/spide/endpoints/`).
-- Spide → Neovim: Spide connects to `v:servername` and calls only the functions in the
-- "RPC API" section below (open, buffer_state, buffer_text, apply_edits, dirty_buffers,
-- save_all, set_user_token). Neovim owns unsaved buffers: edits from Spide are applied as
-- undoable buffer changes guarded by `changedtick`, never by rewriting the file under you.

local M = {}

local uv = vim.uv or vim.loop

M.config = {
  auto_connect = true, -- connect when a buffer inside a Spide workspace is opened
  follow_cursor = false, -- reveal the element under the cursor in Spide on CursorHold
  forward_diagnostics = true, -- send live LSP diagnostics to Spide (evidence for health)
  keymaps = false, -- set to a prefix (e.g. "<leader>S") to install mappings
  timeout_ms = 5000,
}

M.state = {
  endpoint = nil, -- { mcp_url, token, root, host, pid }
  session = nil,
  user_token = nil, -- pushed by Spide over RPC; proves human actions
  id = 0,
}

local function notify(msg, level)
  vim.notify("spide: " .. msg, level or vim.log.levels.INFO)
end

-- ------------------------------------------------------------------ discovery

local function read_json(path)
  local f = io.open(path, "r")
  if not f then
    return nil
  end
  local s = f:read("*a")
  f:close()
  local ok, v = pcall(vim.json.decode, s)
  return ok and v or nil
end

local function alive(pid)
  return pid and pid > 0 and uv.kill(pid, 0) == 0
end

local function real(p)
  return p and (uv.fs_realpath(p) or p) or nil
end

--- The live Spide endpoint whose root contains `path` (deepest root wins).
function M.discover(path)
  path = real(path or vim.fn.getcwd())
  local best
  for dir in vim.fs.parents(path .. "/x") do
    local ep = read_json(dir .. "/.spide/endpoint.json")
    if ep and alive(ep.pid) and ep.mcp_url then
      return ep
    end
  end
  local state = os.getenv("XDG_STATE_HOME") or (os.getenv("HOME") .. "/.local/state")
  local dir = state .. "/spide/endpoints"
  local handle = uv.fs_scandir(dir)
  while handle do
    local name = uv.fs_scandir_next(handle)
    if not name then
      break
    end
    local ep = read_json(dir .. "/" .. name)
    local root = ep and real(ep.root)
    if ep and root and alive(ep.pid) and ep.mcp_url and (path == root or vim.startswith(path, root .. "/")) then
      if not best or #root > #best.root then
        best = ep
      end
    end
  end
  return best
end

-- ------------------------------------------------------------------ HTTP / MCP

local function http_post(url, token, body, timeout)
  local host, port, path = url:match("^http://([^:/]+):(%d+)(/.*)$")
  if not host then
    return nil, "bad url " .. url
  end
  local tcp = uv.new_tcp()
  local chunks, done, err = {}, false, nil
  tcp:connect(host == "localhost" and "127.0.0.1" or host, tonumber(port), function(e)
    if e then
      err, done = e, true
      return
    end
    local req = table.concat({
      "POST " .. path .. " HTTP/1.1",
      "Host: " .. host .. ":" .. port,
      "Content-Type: application/json",
      "Authorization: Bearer " .. token,
      "Content-Length: " .. #body,
      "Connection: close",
      "",
      body,
    }, "\r\n")
    tcp:write(req)
    tcp:read_start(function(re, data)
      if re then
        err = re
      end
      if data then
        table.insert(chunks, data)
      else
        done = true
        tcp:close()
      end
    end)
  end)
  vim.wait(timeout or M.config.timeout_ms, function()
    return done
  end, 10)
  if not done then
    pcall(function()
      tcp:close()
    end)
    return nil, "Spide did not answer"
  end
  if err then
    return nil, tostring(err)
  end
  local resp = table.concat(chunks)
  local status = tonumber(resp:match("^HTTP/%d%.%d (%d+)"))
  local payload = resp:match("\r\n\r\n(.*)$") or ""
  if status ~= 200 then
    return nil, "HTTP " .. tostring(status) .. " " .. payload
  end
  return payload
end

--- Call a Spide tool. `user = true` marks a human action (proven by the user token).
function M.call(tool, args, opts)
  opts = opts or {}
  local ep = M.state.endpoint or M.discover()
  if not ep then
    return nil, "no running Spide for this project (start Spide or `spide-headless serve`)"
  end
  M.state.endpoint = ep
  M.state.id = M.state.id + 1
  local meta = vim.empty_dict()
  if opts.user and M.state.user_token then
    meta = { user_token = M.state.user_token }
  end
  local body = vim.json.encode({
    jsonrpc = "2.0",
    id = M.state.id,
    method = "tools/call",
    params = { name = tool, arguments = args or vim.empty_dict(), _meta = meta },
  })
  local out, err = http_post(ep.mcp_url, ep.token, body, opts.timeout)
  if not out then
    M.state.endpoint = nil -- rediscover next time (Spide restarted?)
    return nil, err
  end
  local ok, v = pcall(vim.json.decode, out)
  if not ok then
    return nil, "bad response"
  end
  if v.error then
    return nil, v.error.message
  end
  local res = v.result or {}
  local text = res.content and res.content[1] and res.content[1].text or ""
  if res.isError then
    return nil, text
  end
  local okj, decoded = pcall(vim.json.decode, text)
  return okj and decoded or text
end

-- ------------------------------------------------------------------ session

local function lsp_names()
  local names = {}
  local get = vim.lsp.get_clients or vim.lsp.get_active_clients
  for _, c in ipairs(get()) do
    names[c.name] = true
  end
  return vim.tbl_keys(names)
end

function M.connect(opts)
  opts = opts or {}
  local ep = M.discover(opts.path)
  if not ep then
    if not opts.quiet then
      notify("no running Spide for " .. (opts.path or vim.fn.getcwd()), vim.log.levels.WARN)
    end
    return false
  end
  M.state.endpoint = ep
  local socket = vim.v.servername
  if socket == nil or socket == "" then
    socket = vim.fn.serverstart()
  end
  local res, err = M.call("editor_register", {
    socket = socket,
    pid = vim.fn.getpid(),
    cwd = vim.fn.getcwd(),
    kind = os.getenv("SPIDE_EMBEDDED") == "1" and "embedded" or "external",
    lsp = lsp_names(),
  })
  if not res then
    if not opts.quiet then
      notify("connect failed: " .. tostring(err), vim.log.levels.ERROR)
    end
    return false
  end
  M.state.session = res.session
  if not opts.quiet then
    notify("connected to Spide (" .. ep.host .. ") for " .. ep.root)
  end
  return true
end

function M.disconnect()
  if M.state.session then
    M.call("editor_unregister", { id = M.state.session.id })
  end
  M.state.session, M.state.endpoint, M.state.user_token = nil, nil, nil
end

local function rel_path(buf)
  local name = real(vim.api.nvim_buf_get_name(buf or 0))
  local ep = M.state.endpoint
  if not name or name == "" or not ep then
    return nil
  end
  local root = real(ep.root)
  if vim.startswith(name, root .. "/") then
    return name:sub(#root + 2)
  end
  return nil
end

-- ------------------------------------------------------------------ user commands

function M.reveal()
  if not M.state.session and not M.connect({ quiet = true }) then
    return notify("not connected", vim.log.levels.WARN)
  end
  local file = rel_path(0)
  if not file then
    return notify("this buffer is outside the Spide workspace", vim.log.levels.WARN)
  end
  local res, err = M.call("reveal", { file = file, line = vim.api.nvim_win_get_cursor(0)[1] })
  if not res then
    return notify(err, vim.log.levels.WARN)
  end
  local function val(x)
    if x == vim.NIL then
      return nil
    end
    return x
  end
  local where = val(res.design_class) or val(res.code_class) or "?"
  if val(res.method) then
    where = where .. "." .. res.method
  end
  notify("revealed " .. where .. (res.in_design and "" or " (not in the design yet)"))
end

local function float(lines, title)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].bufhidden = "wipe"
  local w = 0
  for _, l in ipairs(lines) do
    w = math.max(w, vim.fn.strdisplaywidth(l))
  end
  w = math.min(math.max(w + 2, 30), math.floor(vim.o.columns * 0.8))
  local h = math.min(#lines, math.floor(vim.o.lines * 0.6))
  vim.api.nvim_open_win(buf, true, { relative = "editor", width = w, height = math.max(h, 1), row = 2, col = math.floor((vim.o.columns - w) / 2), border = "rounded", title = title, style = "minimal" })
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = buf, nowait = true })
end

function M.health()
  local res, err = M.call("get_health", {})
  if not res then
    res, err = M.call("get_metrics", {})
  end
  if not res then
    return notify(err, vim.log.levels.WARN)
  end
  local lines = {}
  if type(res) == "table" then
    local report = res.health_report or res
    table.insert(lines, "Evidence · " .. (report.scope or "design"))
    for _, gap in ipairs(report.gaps or {}) do
      table.insert(lines, "  Gap: " .. gap)
    end
    for _, run in ipairs(report.runs or {}) do
      table.insert(lines, (run.tool or "Tool") .. ": " .. (run.status or "unknown"))
    end
    local findings = report.signals or res.issues or {}
    if #findings == 0 then
      table.insert(lines, "No findings from available checks; not a design verdict.")
    end
    for i, issue in ipairs(findings) do
      if i > 12 then
        table.insert(lines, string.format("  … %d more finding(s)", #findings - 12))
        break
      end
      table.insert(lines, "  • " .. (issue.message or ""))
    end
  end
  float(lines, " Spide evidence ")
end

function M.ask(text)
  if not text or text == "" then
    text = vim.fn.input("Spide: ")
  end
  if text == "" then
    return
  end
  local res, err = M.call("request_change", { text = text, file = rel_path(0), line = vim.api.nvim_win_get_cursor(0)[1] }, { user = true })
  notify(res and "request sent" or err, res and vim.log.levels.INFO or vim.log.levels.WARN)
end

function M.trace()
  local res, err = M.call("get_trace", { limit = 40 })
  if not res then
    return notify(err, vim.log.levels.WARN)
  end
  local lines = {}
  for _, r in ipairs(res.rows or {}) do
    table.insert(lines, string.format("%-8s %s", r.actor or "", r.summary or ""))
  end
  float(#lines > 0 and lines or { "(no traced operations)" }, " Spide trace ")
end

function M.patches()
  local res, err = M.call("list_patches", {})
  if not res then
    return notify(err, vim.log.levels.WARN)
  end
  local pending = vim.tbl_filter(function(p)
    return p.state == "pending"
  end, res)
  if #pending == 0 then
    return notify("no pending patches")
  end
  vim.ui.select(pending, {
    prompt = "Spide patch",
    format_item = function(p)
      return p.id .. "  " .. p.file .. "  " .. (p.intent or "")
    end,
  }, function(p)
    if p then
      M.apply_patch(p.id)
    end
  end)
end

function M.apply_patch(id)
  local list = M.call("list_patches", {}) or {}
  for _, p in ipairs(list) do
    if p.id == id then
      local lines = vim.split(p.diff or "", "\n")
      table.insert(lines, 1, p.file .. " — " .. (p.intent or ""))
      if vim.fn.confirm(table.concat(lines, "\n") .. "\n\nApply this patch?", "&Apply\n&Cancel", 2) ~= 1 then
        return
      end
    end
  end
  local res, err = M.call("apply_patch", { id = id }, { user = true })
  notify(res and ("applied " .. id .. (res.via and (" via " .. res.via) or "")) or err, res and vim.log.levels.INFO or vim.log.levels.ERROR)
end

-- ------------------------------------------------------------------ RPC API (called by Spide)

local function buf_for(path)
  local target = real(path)
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) and real(vim.api.nvim_buf_get_name(b)) == target then
      return b
    end
  end
  return nil
end

local function editor_window()
  local cur = vim.api.nvim_get_current_win()
  if vim.api.nvim_win_get_config(cur).relative == "" then
    return cur
  end
  for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if vim.api.nvim_win_get_config(w).relative == "" then
      return w
    end
  end
end

function M.set_user_token(tok)
  M.state.user_token = tok
  return true
end

--- Open `path` at 1-based `line`, 0-based byte `col`.
function M.open(path, line, col)
  local win = editor_window()
  if not win then
    return { ok = false, error = "no editor window" }
  end
  vim.api.nvim_set_current_win(win)
  local b = buf_for(path)
  if b then
    vim.api.nvim_win_set_buf(win, b)
  else
    local ok, e = pcall(vim.cmd.edit, vim.fn.fnameescape(path))
    if not ok then
      return { ok = false, error = tostring(e) }
    end
  end
  local n = vim.api.nvim_buf_line_count(0)
  line = math.max(1, math.min(line or 1, n))
  local text = vim.api.nvim_buf_get_lines(0, line - 1, line, false)[1] or ""
  pcall(vim.api.nvim_win_set_cursor, 0, { line, math.max(0, math.min(col or 0, #text)) })
  vim.cmd("normal! zz")
  pcall(vim.cmd, "redraw")
  return { ok = true, bufnr = vim.api.nvim_get_current_buf() }
end

function M.buffer_state(path)
  local b = buf_for(path)
  if not b then
    return { open = false }
  end
  return { open = true, dirty = vim.bo[b].modified, changedtick = vim.api.nvim_buf_get_changedtick(b) }
end

function M.buffer_text(path)
  local b = buf_for(path)
  if not b then
    return { open = false }
  end
  local lines = vim.api.nvim_buf_get_lines(b, 0, -1, false)
  local text = table.concat(lines, "\n")
  if (vim.bo[b].eol or vim.bo[b].fixeol) and #lines > 0 and not (#lines == 1 and lines[1] == "") then
    text = text .. "\n"
  end
  return { open = true, text = text, changedtick = vim.api.nvim_buf_get_changedtick(b), dirty = vim.bo[b].modified }
end

--- Apply line edits [{start_line,end_line,text}] if the buffer is still at `changedtick`.
function M.apply_edits(path, changedtick, edits)
  local b = buf_for(path)
  if not b then
    return { ok = false, error = "buffer not open" }
  end
  if vim.api.nvim_buf_get_changedtick(b) ~= changedtick then
    return { ok = false, error = "the buffer changed (changedtick " .. vim.api.nvim_buf_get_changedtick(b) .. ")" }
  end
  table.sort(edits, function(a, c)
    return a.start_line > c.start_line
  end)
  local first = true
  vim.api.nvim_buf_call(b, function()
    for _, e in ipairs(edits) do
      local lines = e.text == "" and {} or vim.split(e.text:gsub("\n$", ""), "\n", { plain = true })
      if not first then
        pcall(vim.cmd, "undojoin")
      end
      vim.api.nvim_buf_set_lines(b, e.start_line - 1, e.end_line, false, lines)
      first = false
    end
  end)
  return { ok = true, changedtick = vim.api.nvim_buf_get_changedtick(b) }
end

function M.dirty_buffers()
  local out = {}
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) and vim.bo[b].modified and vim.bo[b].buftype == "" then
      table.insert(out, vim.api.nvim_buf_get_name(b))
    end
  end
  return out
end

function M.save_all()
  pcall(vim.cmd, "silent! wall")
  return M.dirty_buffers()
end

-- ------------------------------------------------------------------ diagnostics forwarding

local pending = {}

local function forward(buf)
  if not M.state.session or not vim.api.nvim_buf_is_valid(buf) then
    return
  end
  local file = rel_path(buf)
  if not file then
    return
  end
  local diags = {}
  for _, d in ipairs(vim.diagnostic.get(buf)) do
    table.insert(diags, {
      lnum = d.lnum,
      col = d.col,
      end_lnum = d.end_lnum or d.lnum,
      end_col = d.end_col or d.col,
      severity = d.severity,
      source = d.source or "",
      code = d.code,
      message = d.message,
    })
  end
  M.call("editor_diagnostics", { file = file, diagnostics = diags, unsaved = vim.bo[buf].modified }, { timeout = 2000 })
end

function M.on_diagnostics(buf)
  if not M.config.forward_diagnostics then
    return
  end
  if pending[buf] then
    pending[buf]:stop()
  else
    pending[buf] = uv.new_timer()
  end
  pending[buf]:start(400, 0, vim.schedule_wrap(function()
    if pending[buf] then
      pending[buf]:close()
      pending[buf] = nil
    end
    forward(buf)
  end))
end

-- ------------------------------------------------------------------ setup

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})
  local group = vim.api.nvim_create_augroup("spide", { clear = true })
  if M.config.auto_connect then
    vim.api.nvim_create_autocmd({ "VimEnter", "BufEnter", "DirChanged" }, {
      group = group,
      callback = function(a)
        if M.state.session then
          return
        end
        local name = vim.api.nvim_buf_get_name(a.buf or 0)
        local path = name ~= "" and vim.fs.dirname(name) or vim.fn.getcwd()
        if M.discover(path) then
          M.connect({ quiet = true, path = path })
        end
      end,
    })
  end
  vim.api.nvim_create_autocmd("DiagnosticChanged", {
    group = group,
    callback = function(a)
      M.on_diagnostics(a.buf)
    end,
  })
  vim.api.nvim_create_autocmd("CursorHold", {
    group = group,
    callback = function()
      if M.config.follow_cursor and M.state.session and rel_path(0) then
        pcall(M.call, "reveal", { file = rel_path(0), line = vim.api.nvim_win_get_cursor(0)[1] }, { timeout = 1000 })
      end
    end,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      if M.state.session then
        pcall(M.call, "editor_unregister", { id = M.state.session.id }, { timeout = 500 })
      end
    end,
  })
  if M.config.keymaps then
    local p = M.config.keymaps
    local map = function(lhs, rhs, desc)
      vim.keymap.set("n", p .. lhs, rhs, { desc = "Spide: " .. desc })
    end
    map("r", M.reveal, "reveal in diagram")
    map("h", M.health, "health")
    map("a", M.ask, "ask for a change")
    map("t", M.trace, "trace")
    map("p", M.patches, "pending patches")
    map("c", M.connect, "connect")
  end
end

return M
