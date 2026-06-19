{
  description = "Hollywood -- pure-Rust video pre-editing automation";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";

    git-hooks.url = "github:cachix/git-hooks.nix";
    git-hooks.inputs.nixpkgs.follows = "nixpkgs";

    devenv.url = "github:cachix/devenv";
    devenv.inputs = {
      nixpkgs.follows = "nixpkgs";
      git-hooks.follows = "git-hooks";
    };

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";

    # GitButler CLI (`but`) + the gitbutler agent skill, packaged for reuse.
    but.url = "github:dataclique/but.nix";
    but.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      git-hooks,
      devenv,
      rust-overlay,
      crane,
      but,
      ...
    }@inputs:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          config.allowUnfreePredicate = pkg: builtins.elem (pkgs.lib.getName pkg) [ "gitbutler-cli" ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        rustPkgs = pkgs.callPackage ./rust.nix { inherit craneLib; };

        ffmpeg = pkgs.ffmpeg_8 or pkgs.ffmpeg;

        hooks = {
          # Nix
          nil.enable = true;
          nixfmt.enable = true;

          # TOML
          taplo.enable = true;

          # Markdown
          denofmt = {
            enable = true;
            name = "denofmt";
            entry = "${pkgs.deno}/bin/deno fmt";
            files = "\\.md$";
            pass_filenames = true;
          };

          # Rust -- custom entry to avoid git-hooks.nix/nixpkgs version mismatch
          rustfmt = {
            enable = true;
            entry = "${rustToolchain}/bin/cargo fmt --";
            files = "\\.rs$";
            pass_filenames = true;
          };
        };

        # Spliced into the gitbutler agent skill's "## This Repository" section.
        repoNotes = ''
          ## This Repository

          - **Pre-commit hooks** run on `but commit`: nixfmt (Nix), rustfmt
            (Rust), taplo (TOML), and `deno fmt` (Markdown). Keep them green.
          - **Commit messages:** conventional, lowercase imperative -- `feat:`,
            `fix:`, `chore:`, `docs:`, `refactor:`, `test:`.
          - **Branch names:** `<type>/<kebab-description>` (e.g.
            `feat/timeline-ir`, `chore/nix-rust-foundation`).
          - **One PR per branch**, stacked with `--anchor`; target 500-1000 lines
            of additions per PR and keep each branch independently buildable.
          - **`master` is protected** -- never push to it directly; land every
            change through stacked draft PRs.

        '';

        devShell = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [
            (but.lib.${system}.devenvModule { inherit repoNotes; })
            ({ ... }: {
              packages = [
                pkgs.pkg-config
                ffmpeg
                pkgs.sqlite
                pkgs.git
              ];

              languages = {
                nix.enable = true;
                rust = {
                  enable = true;
                  toolchain.rustc = rustToolchain;
                  toolchain.cargo = rustToolchain;
                  toolchain.rustfmt = rustToolchain;
                  toolchain.clippy = rustToolchain;
                };
              };

              # ffmpeg-sys-next runs bindgen, which needs libclang at build time.
              env.LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

              git-hooks = { inherit hooks; };

              difftastic.enable = true;
              cachix.enable = true;
            })
          ];
        };

      in
      {
        devShells.default = devShell;

        checks.git-hooks = git-hooks.lib.${system}.run {
          inherit hooks;
          src = self;
        };

        packages = {
          default = rustPkgs.package;
          hollywood = rustPkgs.package;
          hollywood-test = rustPkgs.test;
          hollywood-clippy = rustPkgs.clippy;

          gitbutler-cli = but.packages.${system}.gitbutler-cli;
        };
      }
    );

  nixConfig = {
    extra-substituters = [
      "https://devenv.cachix.org"
      "https://nix-community.cachix.org"
    ];
    extra-trusted-public-keys = [
      "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
    allow-unfree = true;
  };
}
