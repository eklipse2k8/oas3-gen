{
  description = "Flake for oas3-gen";

  inputs = {
    nixpks.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    naersk,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true;
        };
        rust = pkgs.rust-bin.stable.latest.default;
        naerskLib = pkgs.callPackage naersk {
          cargo = rust;
          rustc = rust;
        };

        buildInputs = with pkgs; [
          rust
          pkg-config
          openssl
        ];
        tooling = with pkgs; [
          cargo-nextest
          cargo-mutants # Rust code mutation testing
          alejandra # Nix code formatter
          deadnix # Nix Dead code detection
          statix # Nix static checks
          taplo # Toml toolkit and formatter
        ];
      in
        with pkgs; {
          # Build the packages with `nix build` or `nix build .#oas3-gen` for example.
          packages = rec {
            default = oas3-gen;
            oas3-gen = naerskLib.buildPackage {
              pname = "oas3-gen";
              src = ./.;
              inherit buildInputs;
            };
          };
          # Run the packages with `nix run` or `nix run .#oas3-gen` for example.
          apps = rec {
            default = oas3-gen;
            oas3-gen = flake-utils.lib.mkApp {
              drv = self.packages.${system}.oas3-gen;
            };
          };
          # Enter the reproducible development shell using `nix develop` (automatically done with `direnv allow` if available)
          devShells.default = mkShell {
            buildInputs = buildInputs ++ tooling;
          };
        }
    );
}
