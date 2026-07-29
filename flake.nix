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
        ];

        nightlyPkgs = [];

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
              version = "0.1.0";
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
