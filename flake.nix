{
  description = "A simple Rust project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        rustVersion = pkgs.rust-bin.stable.latest.default;
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in
      with pkgs;
      {
        devShell = pkgs.mkShell {
          buildInputs = [ (rustVersion.override { extensions = [ "rust-src" ]; }) ];

          packages = [
            rust-analyzer
            clippy
            rustfmt
            bacon
          ];

          RUST_BACKTRACE = 1;

          shellHook = ''
            echo "Welcome to the Rust dev environment"
          '';
        };
      }
    );
}
