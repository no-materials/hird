{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };

    rust-overlay.url = "github:oxalica/rust-overlay";

    ticket.url = "github:wedow/ticket";
    ticket.flake = false;
  };

  outputs = inputs @ {
    flake-parts,
    nixpkgs,
    rust-overlay,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = inputs.flake-utils.lib.defaultSystems;

      perSystem = {
        self',
        system,
        ...
      }: let
        # Import nixpkgs with the rust overlay.
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };

        # LLVM toolchain pieces we’ll use for wasm C code.
        clangWasm = pkgs.llvmPackages_latest.clang-unwrapped;
        bintoolsWasm = pkgs.llvmPackages_latest.bintools-unwrapped;

        # The nx rust toolchain - sourced from `rust-toolchain.toml`.
        nxRust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # A musl-target override of the above toolchain.
        nxRustMusl = nxRust.override {
          targets = ["x86_64-unknown-linux-musl"];
        };

        # A nightly rust toolchain provided as an alternative.
        nightlyRust = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {});

        ticket = pkgs.stdenv.mkDerivation {
          pname = "ticket";
          version = inputs.ticket.rev;
          src = inputs.ticket;
          dontBuild = true;
          installPhase = ''
            mkdir -p $out/bin
            cp ticket $out/bin/tk
            chmod +x $out/bin/tk
          '';
        };

        # Rust packages - included in all shells.
        generalPkgs = with pkgs; [
          curl
          unzip
          git-lfs
          pkg-config
          openssl
          vulkan-loader
          wayland
          libxkbcommon
          graphviz
          fontconfig.lib
          patchelf
          cargo-generate

          # In-repo ticket system
          ticket

          # New: tools for wasm C compilation
          clangWasm
          bintoolsWasm

          # BEAM toolchain: erlc/erl/escript/dialyzer for Erlang codegen tests
          erlang

          # Tree-sitter grammar work: `tree-sitter generate` needs node to
          # evaluate grammar.js.
          tree-sitter
          nodejs
        ];

        nightlyPkgs = [];

        # Package versions are read from the manifests that own them rather
        # than restated here, so a release bumps one number per artefact.
        workspaceVersion =
          (builtins.fromTOML (builtins.readFile ./Cargo.toml))
          .workspace
          .package
          .version;
        grammarVersion =
          (builtins.fromJSON (builtins.readFile ./tree-sitter-hird/tree-sitter.json))
          .metadata
          .version;

        # Every `.hird` source the repository ships. The tree-sitter grammar
        # is checked against all of them.
        hirdSources = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            (pkgs.lib.fileset.fileFilter (file: file.hasExt "hird") ./demo)
            (pkgs.lib.fileset.fileFilter (file: file.hasExt "hird") ./crates)
          ];
        };

        mkName = name: name + "-dev-shell";
        mkPersonalShell = {
          shellName,
          shellPackages,
        }:
          pkgs.mkShell rec {
            name = mkName shellName;
            packages = generalPkgs ++ shellPackages;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath packages;

            # --- WASM zstd bits start here ---

            # Use clang as the C compiler for wasm32-unknown-unknown.
            CC_wasm32_unknown_unknown = "${clangWasm}/bin/clang";

            # Use llvm-ar as the archiver for wasm32-unknown-unknown.
            AR_wasm32_unknown_unknown = "${bintoolsWasm}/bin/llvm-ar";

            # (optional but sometimes handy if headers can't be found)
            # CFLAGS_wasm32_unknown_unknown = let
            #   libPath = pkgs.lib.getLib clangWasm;
            #   major = pkgs.lib.versions.major clangWasm.version;
            # in "-I${libPath}/lib/clang/${major}/include";
          };
      in {
        formatter = pkgs.nixfmt-rfc-style;

        packages = {
          default = self'.packages.hird-lsp;

          # The LSP server as an installable package, built with the pinned
          # toolchain. Consumers (editor configs) take this flake as an input
          # and reference `packages.<system>.hird-lsp`.
          hird-lsp = let
            rustPlatform = pkgs.makeRustPlatform {
              cargo = nxRust;
              rustc = nxRust;
            };
          in
            rustPlatform.buildRustPackage {
              pname = "hird-lsp";
              version = workspaceVersion;
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = ["-p" "hird-lsp"];
              # The workspace test suite runs in CI; the package just ships
              # the binary.
              doCheck = false;
              meta = {
                description = "LSP server for Hirð";
                mainProgram = "hird-lsp";
              };
            };

          # The MCP server as an installable package. Consumers (LLM agent
          # frameworks, MCP client configs) take this flake as an input and
          # reference `packages.<system>.hird-mcp`.
          hird-mcp = let
            rustPlatform = pkgs.makeRustPlatform {
              cargo = nxRust;
              rustc = nxRust;
            };
          in
            rustPlatform.buildRustPackage {
              pname = "hird-mcp";
              version = workspaceVersion;
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = ["-p" "hird-mcp"];
              # The workspace test suite runs in CI; the package just ships
              # the binary.
              doCheck = false;
              meta = {
                description = "MCP server for Hirð compiler introspection";
                mainProgram = "hird-mcp";
              };
            };

          # The tree-sitter grammar as an installable parser, with its query
          # files under `$out/queries`. Editor configs (nvim-treesitter, nvf)
          # take this flake as an input and reference
          # `packages.<system>.tree-sitter-hird`.
          tree-sitter-hird = pkgs.tree-sitter.buildGrammar {
            language = "hird";
            version = grammarVersion;
            src = ./tree-sitter-hird;
            # `src/parser.c` is generated, not committed.
            generate = true;
            meta = {
              description = "Tree-sitter grammar and highlight queries for Hirð";
            };
          };
        };

        checks = {
          # The grammar's corpus tests, then a zero-ERROR parse of every
          # `.hird` source the repository ships, then every query file run
          # against them so a stale node name cannot ship silently.
          tree-sitter-hird =
            pkgs.runCommandCC "tree-sitter-hird-check" {
              nativeBuildInputs = [pkgs.tree-sitter pkgs.nodejs];
            } ''
              cp -r ${./tree-sitter-hird} grammar
              chmod -R u+w grammar
              cd grammar
              export HOME=$PWD/.home

              tree-sitter generate
              tree-sitter test

              sources=$(find ${hirdSources} -name '*.hird')
              tree-sitter parse --quiet $sources
              for query in queries/*.scm; do
                tree-sitter query --quiet "$query" $sources
              done

              touch $out
            '';
        };

        devShells.default = self'.devShells.rust-nx;
        devShells = {
          rust-nx = mkPersonalShell {
            shellName = "rust-nx";
            shellPackages = [nxRust];
          };
          rust-nightly = mkPersonalShell {
            shellName = "rust-nightly";
            shellPackages = nightlyPkgs ++ [nightlyRust];
          };
          rust-musl = mkPersonalShell {
            shellName = "rust-musl";
            shellPackages = [
              nxRustMusl
              pkgs.pkgsCross.musl64.stdenv.cc
            ];
          };
        };
      };
    };
}
