{
  description = "Orthanc - A Jellyfin replacement in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Fenix toolchain - latest stable Rust
        toolchain = fenix.packages.${system}.stable.toolchain;

      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            toolchain
            sqlite
            sqlx-cli
            dioxus-cli
            cargo-watch
            cargo-edit
            cargo-expand
            bacon
            pkg-config
            openssl
            ffmpeg
            just
            ripgrep
            fd
          ];

          # SQLite configuration for development
          DATABASE_URL = "sqlite:./orthanc.db";
        };
      }
    );
}
