# Editor setup

How to wire an editor to the Hirð toolchain: the language server, and
the tree-sitter grammar for syntax highlighting.

## Language server

`hird-lsp` is a Language Server Protocol server over the compiler front
end, speaking stdio. Point any LSP client at the `hird-lsp` binary, with
no arguments.

v0.1 capabilities:

- **Diagnostics** on file open and save: parse errors, then type errors
  and warnings, with source spans.
- **Hover**: the inferred type of the identifier or expression under the
  cursor, including the effect row for functions
  (`read_file : Path → String ! {Tool<ReadFile>}`).
- **Go-to-definition** for top-level declarations: functions, types and
  their constructors, effects, tools (by marker or generated function
  name), actors and their message types, and supervisors.

Known limitations (real, by design for v0.1):

- No completion, rename/refactor, or code actions.
- No workspace-wide analysis: each file is compiled alone, so `use`
  imports of other modules report as unresolved and definitions resolve
  only within the current file.
- No incremental compilation: every change recompiles the whole file.

## Syntax highlighting

`tree-sitter-hird/` is a tree-sitter grammar for the v0.1 surface, with
`highlights.scm`, `indents.scm` and `folds.scm` under `queries/`. Both
operator spellings parse identically, so `→` and `->` highlight the same.
The flake builds it as a package output, next to `hird-lsp`:

```sh
nix build github:no-materials/hird#tree-sitter-hird
```

The result holds the compiled `parser` and a copy of `queries/`. A
flake-based Neovim configuration takes this repository as an input and
hands the grammar to nvim-treesitter, which wants the parser and the
queries under the names it looks them up by:

```nix
# inputs.hird.url = "github:no-materials/hird";
{
  plugins = [
    (pkgs.neovimUtils.grammarToPlugin
      inputs.hird.packages.${pkgs.system}.tree-sitter-hird)
  ];
}
```

Neovim needs the file type registered too, whichever route below you
take, since `.hird` is not one it knows:

```lua
vim.filetype.add({ extension = { hird = "hird" } })
```

Without nix, nvim-treesitter builds the grammar itself, given the
tree-sitter CLI on `PATH` (`npm i -g tree-sitter-cli`). `src/parser.c`
is generated rather than committed — `grammar.js` is the only source —
so `requires_generate_from_grammar` is the part that matters: it makes
`:TSInstall hird` generate the parser before compiling it.

```lua
require('nvim-treesitter.parsers').get_parser_configs().hird = {
  install_info = {
    url = "https://github.com/no-materials/hird",
    location = "tree-sitter-hird",
    files = { "src/parser.c", "src/scanner.c" },
    requires_generate_from_grammar = true,
  },
  filetype = "hird",
}
```

That installs the parser but not the queries: nvim-treesitter ships
those only for the languages it supports, so copy this grammar's onto
the runtime path by hand. It is the one step the nix package does for
you.

```sh
mkdir -p ~/.config/nvim/queries/hird
cp tree-sitter-hird/queries/*.scm ~/.config/nvim/queries/hird/
```

With no plugin at all, build the parser straight onto the runtime path
next to those queries (`tree-sitter build -o ~/.config/nvim/parser/hird.so`)
and call `vim.treesitter.start()` from a `FileType hird` autocommand.

Working on the grammar itself needs no global tree-sitter CLI — the dev
shell ships one, and `nix flake check` runs the corpus tests and parses
every `.hird` source in the repository:

```sh
cd tree-sitter-hird && tree-sitter generate && tree-sitter test
```
