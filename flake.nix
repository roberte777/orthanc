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

        # Fenix toolchain - latest stable Rust with wasm32 target
        toolchain = fenix.packages.${system}.combine [
          fenix.packages.${system}.stable.toolchain
          fenix.packages.${system}.targets.wasm32-unknown-unknown.stable.rust-std
        ];

        # Build wasm-bindgen-cli with the correct version (0.2.118)
        wasm-bindgen-cli = pkgs.rustPlatform.buildRustPackage rec {
          pname = "wasm-bindgen-cli";
          version = "0.2.118";

          src = pkgs.fetchCrate {
            inherit pname version;
            sha256 = "sha256-ve783oYH0TGv8Z8lIPdGjItzeLDQLOT5uv/jbFOlZpI=";
          };

          cargoHash = "sha256-EYDfuBlH3zmTxACBL+sjicRna84CvoesKSQVcYiG9P0=";

          nativeBuildInputs = [ pkgs.pkg-config ];

          buildInputs = [ pkgs.openssl pkgs.curl ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];

          doCheck = false;
        };

      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = (with pkgs; [
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
            trunk
            binaryen
          ]) ++ [ wasm-bindgen-cli ];

          # SQLite configuration for development
          DATABASE_URL = "sqlite:./orthanc.db";
        };
      }
    );
}
