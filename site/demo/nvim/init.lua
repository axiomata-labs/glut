-- Neovim as scene 8 shows it: glut.nvim (a package the recorder put in this home) and Glut on the take's model,
-- in a bit less than half of the screen.
vim.opt.swapfile = false
vim.opt.number = true
vim.opt.wrap = false
vim.opt.expandtab = true
vim.opt.shiftwidth = 4
vim.opt.tabstop = 4
vim.opt.autoindent = true
vim.opt.termguicolors = true
vim.opt.showtabline = 0
vim.opt.laststatus = 3
-- The plugin's flame, and the file in front — a terminal's long internal name says nothing.
vim.opt.statusline = " %{v:lua.require'glut'.status()}   %{&buftype ==# 'terminal' ? '' : expand('%:t')}%=%l:%c "

require("glut").setup({
  cmd = { "glut", "--provider", "anthropic", "--model", "claude-sonnet-5-5", "--lang", "en" },
  split = { side = "right", width = 0.45 },
})
