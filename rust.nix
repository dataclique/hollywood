{ pkgs, craneLib }:

let
  inherit (pkgs) lib;

  # ffmpeg-next 8.x links against FFmpeg 8 headers; prefer the explicitly
  # versioned attr and fall back to the channel default if it is already 8.x.
  ffmpeg = pkgs.ffmpeg_8 or pkgs.ffmpeg;

  # Keep clippy.toml alongside the standard Cargo sources so the lint
  # derivation honours the in-tree allow-list (unwrap/expect/indexing in tests).
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type: (craneLib.filterCargoSources path type) || (lib.hasSuffix "clippy.toml" path);
  };

  commonArgs = {
    pname = "hollywood";
    version = "0.1.0";
    inherit src;
    strictDeps = true;

    # pkg-config locates the FFmpeg libraries; bindgenHook supplies libclang +
    # clang args so ffmpeg-sys-next can generate its bindings.
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.rustPlatform.bindgenHook
    ];

    buildInputs = [
      ffmpeg
      pkgs.sqlite
    ]
    ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk_15 ];

    RUSTFLAGS = "-D warnings";
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

in
{
  package = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      doCheck = true;
    }
  );

  # CI check derivations -- lighter than buildPackage (no final link step).
  test = craneLib.cargoTest (
    commonArgs
    // {
      inherit cargoArtifacts;
      cargoTestExtraArgs = "--workspace";
    }
  );

  clippy = craneLib.cargoClippy (
    commonArgs
    // {
      inherit cargoArtifacts;
      cargoClippyExtraArgs = "--workspace --all-targets -- -D clippy::all";
    }
  );
}
