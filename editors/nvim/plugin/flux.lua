local flux = {}

local function project_root(path)
    local dir = vim.fs.dirname(path)
    local manifest = vim.fs.find({ "flux.toml", ".git" }, { path = dir, upward = true })[1]
    if manifest then
        if vim.fs.basename(manifest) == "flux.toml" then
            return vim.fs.dirname(manifest)
        end
        return vim.fs.dirname(manifest)
    end
    return dir
end

local function fluxc_path()
    if type(vim.g.fluxc_path) == "string" and vim.g.fluxc_path ~= "" then
        return vim.g.fluxc_path
    end
    return "flux"
end

local function lsp_command()
    if type(vim.g.flux_lsp_cmd) == "table" and #vim.g.flux_lsp_cmd > 0 then
        return vim.g.flux_lsp_cmd
    end
    if type(vim.g.flux_lsp_cmd) == "string" and vim.g.flux_lsp_cmd ~= "" then
        return { vim.g.flux_lsp_cmd, "lsp" }
    end
    return { fluxc_path(), "lsp" }
end

local function start_lsp(bufnr)
    local name = vim.api.nvim_buf_get_name(bufnr)
    if name == "" then
        return
    end
    vim.lsp.start({
        name = "flux",
        cmd = lsp_command(),
        root_dir = project_root(name),
        filetypes = { "flux" },
    }, { bufnr = bufnr })
end

vim.filetype.add({
    extension = {
        flux = "flux",
    },
})

vim.api.nvim_create_user_command("FluxRun", function(opts)
    local target = opts.args ~= "" and opts.args or vim.api.nvim_buf_get_name(0)
    if target == "" then
        vim.notify("FluxRun requires a saved .flux file or explicit target", vim.log.levels.ERROR)
        return
    end
    vim.cmd("botright 12split")
    vim.cmd("terminal " .. vim.fn.shellescape(fluxc_path()) .. " run " .. vim.fn.shellescape(target))
end, {
    nargs = "?",
    complete = "file",
    desc = "Run a Flux target with automatic save-triggered rebuilds",
})

local group = vim.api.nvim_create_augroup("FluxLanguage", { clear = true })
vim.api.nvim_create_autocmd("FileType", {
    group = group,
    pattern = "flux",
    callback = function(args)
        start_lsp(args.buf)
    end,
})

flux.start_lsp = start_lsp
return flux
