-- RingMD's bundled Neovim config (F2.R2, F8).
--
-- Loaded with `nvim --embed -u <this file>`, and RingMD starts Neovim with
-- NVIM_APPNAME=ringmd, so your own Neovim config, plugins, shada, swap and
-- undo files are never touched. Small tweaks go in
-- `~/.config/ringmd/init.lua`, which is sourced at the end (F8.R4).

local api = vim.api

-- `-u` does not set $MYVIMRC; point it here so `:echo $MYVIMRC` is honest.
local this_file = debug.getinfo(1, 'S').source:sub(2)
vim.env.MYVIMRC = this_file
local runtime_dir = vim.fn.fnamemodify(this_file, ':p:h')

---------------------------------------------------------------------------
-- Isolation (F2.R2, T2.1)
---------------------------------------------------------------------------

-- Drop the user and system config/data directories from 'runtimepath' and
-- 'packpath', so no plugin/, ftplugin/ or pack/ directory outside Neovim's
-- own runtime is loaded. Keeps $VIMRUNTIME and the parser directory.
local function is_user_dir(path)
  if vim.startswith(path, vim.env.VIMRUNTIME) then
    return false
  end
  local roots = { vim.fn.stdpath('config'), vim.fn.stdpath('data') }
  vim.list_extend(roots, vim.fn.stdpath('config_dirs'))
  vim.list_extend(roots, vim.fn.stdpath('data_dirs'))
  for _, root in ipairs(roots) do
    if path == root or vim.startswith(path, root .. '/') then
      return true
    end
  end
  return false
end

local function clean_path(option)
  local kept = vim.tbl_filter(function(path)
    return not is_user_dir(path)
  end, vim.opt[option]:get())
  vim.opt[option] = kept
end

clean_path('runtimepath')
clean_path('packpath')
vim.opt.runtimepath:append(runtime_dir)

---------------------------------------------------------------------------
-- Markdown defaults (F8.R1)
---------------------------------------------------------------------------

-- The markdown ftplugin otherwise forces shiftwidth=4.
vim.g.markdown_recommended_style = 0

vim.opt.termguicolors = true
vim.opt.mouse = 'a'
vim.opt.wrap = true
vim.opt.linebreak = true
vim.opt.breakindent = true
vim.opt.spell = false
vim.opt.conceallevel = 0
vim.opt.expandtab = true
vim.opt.shiftwidth = 2
vim.opt.softtabstop = 2
vim.opt.number = false
vim.opt.relativenumber = false

local group = api.nvim_create_augroup('ringmd', { clear = true })

-- RingMD is a Markdown editor: new unnamed buffers are Markdown.
api.nvim_create_autocmd({ 'VimEnter', 'BufEnter' }, {
  group = group,
  callback = function(args)
    local bo = vim.bo[args.buf]
    if bo.buftype == '' and bo.filetype == '' and api.nvim_buf_get_name(args.buf) == '' then
      bo.filetype = 'markdown'
    end
  end,
})

-- Treesitter highlighting when the parser is available (F8.R3). Neovim
-- 0.10 ships the markdown parsers; otherwise regex syntax stays on.
api.nvim_create_autocmd('FileType', {
  group = group,
  pattern = 'markdown',
  callback = function(args)
    if pcall(vim.treesitter.language.add, 'markdown') then
      pcall(vim.treesitter.start, args.buf, 'markdown')
    end
  end,
})

---------------------------------------------------------------------------
-- Talking to the GUI
---------------------------------------------------------------------------

-- The GUI's RPC channel: the attached UI.
local function gui_channel()
  for _, ui in ipairs(api.nvim_list_uis()) do
    if ui.chan and ui.chan > 0 then
      return ui.chan
    end
  end
  return nil
end

local function notify_gui(method, ...)
  local chan = gui_channel()
  if chan then
    vim.rpcnotify(chan, method, ...)
  end
end

-- System clipboard through the GUI (F2.R8): no xclip/wl-copy needed.
local function copy(lines, regtype)
  notify_gui('ringmd_clipboard_set', lines, regtype)
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
  notify_gui('ringmd_buffer', {
    name = api.nvim_buf_get_name(0),
    modified = vim.bo.modified,
  })
end

api.nvim_create_autocmd(
  { 'VimEnter', 'BufEnter', 'BufFilePost', 'BufWritePost', 'BufModifiedSet' },
  { group = group, callback = report_buffer }
)

-- Startup is done (your init.lua included): the GUI applies the theme now.
api.nvim_create_autocmd('VimEnter', {
  group = group,
  callback = function()
    notify_gui('ringmd_ready')
  end,
})

---------------------------------------------------------------------------
-- Live preview feed (F4.R2, F4.R4)
---------------------------------------------------------------------------

-- The current buffer's text goes to the GUI after edits settle for
-- `g:ringmd_preview_debounce` ms (default 50), and right away when another
-- buffer is entered. Each message carries the whole text: simple, and
-- cheap for documents people write by hand.
vim.g.ringmd_preview_debounce = vim.g.ringmd_preview_debounce or 50

local function send_text(buf)
  if not api.nvim_buf_is_valid(buf) or buf ~= api.nvim_get_current_buf() then
    return
  end
  local lines = api.nvim_buf_get_lines(buf, 0, -1, false)
  notify_gui('ringmd_text', {
    buf = buf,
    text = table.concat(lines, '\n') .. '\n',
  })
end

local timer = vim.uv.new_timer()
local attached = {}

local function schedule_text(buf)
  timer:stop()
  timer:start(vim.g.ringmd_preview_debounce, 0, vim.schedule_wrap(function()
    send_text(buf)
  end))
end

local function attach(buf)
  if attached[buf] then
    return
  end
  attached[buf] = api.nvim_buf_attach(buf, false, {
    on_lines = function(_, b)
      schedule_text(b)
    end,
    on_reload = function(_, b)
      schedule_text(b)
    end,
    on_detach = function(_, b)
      attached[b] = nil
    end,
  }) or nil
end

local last_cursor_line = nil

local function send_cursor(force)
  local line = api.nvim_win_get_cursor(0)[1]
  if force or line ~= last_cursor_line then
    last_cursor_line = line
    notify_gui('ringmd_cursor', line)
  end
end

api.nvim_create_autocmd({ 'VimEnter', 'BufEnter' }, {
  group = group,
  callback = function(args)
    attach(args.buf)
    send_text(args.buf)
    send_cursor(true)
  end,
})

api.nvim_create_autocmd({ 'CursorMoved', 'CursorMovedI' }, {
  group = group,
  callback = function()
    send_cursor(false)
  end,
})

---------------------------------------------------------------------------
-- User commands (F8.R2)
---------------------------------------------------------------------------

-- Gruvbox variants (F5.R1) and `system` (follow the desktop, F5.R5).
-- Must match `model::theme::ThemeChoice::names()`.
local THEMES = {
  'dark-hard',
  'dark-medium',
  'dark-soft',
  'light-hard',
  'light-medium',
  'light-soft',
  'system',
  'system-hard',
  'system-soft',
}

api.nvim_create_user_command('Preview', function()
  notify_gui('ringmd_preview')
end, { nargs = 0, desc = 'Show or hide the Markdown preview' })

api.nvim_create_user_command('ExportPdf', function(opts)
  local path = vim.NIL
  if opts.args ~= '' then
    path = vim.fn.fnamemodify(vim.fn.expand(opts.args), ':p')
  end
  notify_gui('ringmd_export_pdf', path)
end, { nargs = '?', complete = 'file', desc = 'Export the buffer to PDF' })

api.nvim_create_user_command('Theme', function(opts)
  if not vim.tbl_contains(THEMES, opts.args) then
    api.nvim_err_writeln(
      ('Unknown theme "%s". Choose one of: %s'):format(opts.args, table.concat(THEMES, ', '))
    )
    return
  end
  notify_gui('ringmd_theme', opts.args)
end, {
  nargs = 1,
  desc = 'Switch the Gruvbox variant',
  complete = function(lead)
    return vim.tbl_filter(function(name)
      return vim.startswith(name, lead)
    end, THEMES)
  end,
})

-- GUI shortcuts, Normal mode only (D8); other modes keep Neovim's meaning.
vim.keymap.set('n', '<C-p>', '<Cmd>Preview<CR>', { desc = 'Toggle the preview' })

---------------------------------------------------------------------------
-- Your tweaks (F8.R4): ~/.config/ringmd/init.lua, after everything above.
---------------------------------------------------------------------------

local user_init = vim.fn.stdpath('config') .. '/init.lua'
if user_init ~= this_file and vim.fn.filereadable(user_init) == 1 then
  local ok, err = pcall(vim.cmd.source, vim.fn.fnameescape(user_init))
  if not ok then
    vim.schedule(function()
      api.nvim_err_writeln(('Error in %s: %s'):format(user_init, err))
    end)
  end
end
