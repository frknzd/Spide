if vim.g.loaded_spide then
  return
end
vim.g.loaded_spide = true

local function spide()
  return require("spide")
end

local cmds = {
  SpideConnect = { function() spide().connect() end, "Connect this Neovim to the Spide owning the project" },
  SpideDisconnect = { function() spide().disconnect() end, "Detach from Spide" },
  SpideReveal = { function() spide().reveal() end, "Show the element under the cursor in Spide's diagram" },
  SpideHealth = { function() spide().health() end, "Show the project's design health" },
  SpideTrace = { function() spide().trace() end, "Show the compact operation trace" },
  SpidePatches = { function() spide().patches() end, "Review and apply pending patches" },
}
for name, c in pairs(cmds) do
  vim.api.nvim_create_user_command(name, c[1], { desc = c[2] })
end
vim.api.nvim_create_user_command("SpideAsk", function(o)
  spide().ask(o.args)
end, { nargs = "*", desc = "Ask Spide/the agent for a change here" })
vim.api.nvim_create_user_command("SpideApplyPatch", function(o)
  spide().apply_patch(o.args)
end, { nargs = 1, desc = "Apply a staged patch" })
vim.api.nvim_create_user_command("SpideFollow", function()
  local c = spide().config
  c.follow_cursor = not c.follow_cursor
  vim.notify("spide: follow cursor " .. (c.follow_cursor and "on" or "off"))
end, { desc = "Toggle revealing the cursor's element in Spide" })
