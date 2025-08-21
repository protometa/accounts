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
        # rustPlatform = pkgs.makeRustPlatform {
        #   cargo = rustVersion;
        #   rustc = rustVersion;
        # };
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustVersion = pkgs.rust-bin.stable."1.88.0".default;
      in
      with pkgs;
      {
        devShell = pkgs.mkShell {
          buildInputs = [
            (rustVersion.override {
              extensions = [
                "rust-src"
                "rust-analyzer"
              ];
            })
          ];

          packages = [
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
