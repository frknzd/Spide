# spide.nvim

Neovim as [Spide](../../README.md)'s editor: navigate between the design diagram and code in both
directions, forward live LSP diagnostics as health evidence, and review the agent's patches —
without Spide ever touching your unsaved buffers behind your back.

## Install

lazy.nvim / LazyVim:

```lua
{
  dir = "~/Projects/Spide/integrations/nvim",
  name = "spide.nvim",
  lazy = false,
  opts = { keymaps = "<leader>j" }, -- optional; commands work without keymaps
}
```

Spide's embedded Neovim loads the same plugin automatically.

## Use

| Command | |
|---|---|
| `:SpideConnect` / `:SpideDisconnect` | attach to the Spide (desktop or `spide-headless serve`) owning this project — automatic by default |
| `:SpideReveal` | select the element under the cursor on Spide's diagram |
| `:SpideFollow` | toggle revealing on `CursorHold` |
| `:SpideAsk [text]` | send a change request with this location to the agent |
| `:SpideHealth` | the project's design evidence (findings, analysis status, gaps) |
| `:SpideTrace` | what agents changed, one line per operation |
| `:SpidePatches` / `:SpideApplyPatch {id}` | review (diff) and apply a staged agent patch |

With `keymaps = "<leader>j"`: `jr` reveal · `jh` health · `ja` ask · `jt` trace · `jp` patches · `jc` connect.

Options: `auto_connect`, `follow_cursor`, `forward_diagnostics`, `keymaps`, `timeout_ms`.

## How it works

- **Discovery:** the plugin reads `.spide/endpoint.json` (or `$XDG_STATE_HOME/spide/endpoints/`) for
  the MCP URL and bearer token of the live Spide process owning the workspace.
- **Registration:** it registers `v:servername`; Spide connects to that socket and calls only the
  plugin's fixed RPC functions (`open`, `buffer_state`, `buffer_text`, `apply_edits`,
  `dirty_buffers`, `save_all`, `set_user_token`).
- **Edits** from Spide are applied with `nvim_buf_set_lines` as one undo step, only if the buffer's
  `changedtick` is the one the edit was computed against.
- **User token:** Spide pushes a token over RPC so actions you take here (`:SpideApplyPatch`) count
  as yours, even when agents are read-only. It is never written to disk.

## Tests

```sh
nvim --headless --clean -l integrations/nvim/tests/run.lua
cargo test -p spide-headless --test nvim   # real Neovim + spide-headless end to end
```
