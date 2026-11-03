-- RingMD's bundled Neovim config (F2.R2, F8).
-- Loaded with `nvim --embed -u <this file>`; your own init.lua is not read.
-- M2 grows this into the full clean config (T2.1–T2.4).

-- `-u` does not set $MYVIMRC; point it here so `:echo $MYVIMRC` is honest.
vim.env.MYVIMRC = debug.getinfo(1, 'S').source:sub(2)

vim.opt.termguicolors = true
vim.opt.mouse = 'a'

-- The GUI's RPC channel: the attached UI.
local function gui_channel()
  for _, ui in ipairs(vim.api.nvim_list_uis()) do
    if ui.chan and ui.chan > 0 then
      return ui.chan
    end
  end
  return nil
end

-- System clipboard through the GUI (F2.R8): no xclip/wl-copy needed.
local function copy(lines, regtype)
  local chan = gui_channel()
  if chan then
    vim.rpcnotify(chan, 'ringmd_clipboard_set', lines, regtype)
  end
end

local function paste()
  local chan = gui_channel()
  if not chan then
    return { {}, 'v' }
  end
  return vim.rpcrequest(chan, 'ringmd_clipboard_get')
end

vim.g.clipboard = {
  name = 'ringmd',
  copy = { ['+'] = copy, ['*'] = copy },
  paste = { ['+'] = paste, ['*'] = paste },
  cache_enabled = 0,
}

-- Keep the window title and modified flag in sync (F1.R4).
local function report_buffer()
  local chan = gui_channel()
  if chan then
    vim.rpcnotify(chan, 'ringmd_buffer', {
      name = vim.api.nvim_buf_get_name(0),
      modified = vim.bo.modified,
    })
  end
end

vim.api.nvim_create_autocmd(
  { 'VimEnter', 'BufEnter', 'BufFilePost', 'BufWritePost', 'BufModifiedSet' },
  { group = vim.api.nvim_create_augroup('ringmd', { clear = true }), callback = report_buffer }
)
