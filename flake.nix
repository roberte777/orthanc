{
  description = "Orthanc - A Jellyfin replacement in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Auto-generated Android SDK packages — tracks Google's repository daily so
    # hashes stay fresh. Avoids the stale-hash problem in nixpkgs' androidenv.
    android-nixpkgs = {
      url = "github:tadfisher/android-nixpkgs";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix, android-nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # Allow unfree (Android SDK has unfree components)
        pkgs = import nixpkgs {
          inherit system;
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };

        isDarwin = pkgs.stdenv.isDarwin;

        # Android NDK version pinned to what Dioxus mobile docs reference.
        # If you change this, also update the `ndk-25-2-9519653` attr below and NDK_HOME.
        # (android-nixpkgs uses dashes in attr names: 25.2.9519653 → 25-2-9519653.)
        ndkVersion = "25.2.9519653";

        androidSdk = android-nixpkgs.sdk.${system} (sdkPkgs: with sdkPkgs; [
          cmdline-tools-latest
          platform-tools
          build-tools-34-0-0
          platforms-android-34
          platforms-android-33
          ndk-25-2-9519653
        ]);

        # Fenix toolchain — stable Rust + wasm32 + mobile targets.
        # iOS targets are macOS-only (the toolchain still installs but cargo can't link without Xcode).
        fenixPkgs = fenix.packages.${system};

        baseTargets = [
          fenixPkgs.stable.toolchain
          fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
          # Android targets — needed on every host that builds for Android.
          fenixPkgs.targets.aarch64-linux-android.stable.rust-std
          fenixPkgs.targets.armv7-linux-androideabi.stable.rust-std
          fenixPkgs.targets.x86_64-linux-android.stable.rust-std
          fenixPkgs.targets.i686-linux-android.stable.rust-std
          fenixPkgs.rust-analyzer
        ];

        iosTargets = pkgs.lib.optionals isDarwin [
          fenixPkgs.targets.aarch64-apple-ios.stable.rust-std
          fenixPkgs.targets.aarch64-apple-ios-sim.stable.rust-std
        ];

        toolchain = fenixPkgs.combine (baseTargets ++ iosTargets);

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
            process-compose
            # JDK for Android builds (Gradle requires a JDK; 17 is the current Android Gradle Plugin baseline)
            jdk17
            # Android SDK + NDK (from android-nixpkgs flake)
            androidSdk
          ]) ++ [ wasm-bindgen-cli ];

          # SQLite configuration for development
          DATABASE_URL = "sqlite:./orthanc.db";

          # process-compose's API/TUI defaults to 8080, which clashes with orthanc_server.
          PC_PORT_NUM = "8085";

          # Android toolchain env vars — dx CLI looks for NDK_HOME (per Dioxus mobile docs).
          # ANDROID_SDK_ROOT/ANDROID_NDK_ROOT are kept in sync for tools that read either name.
          # android-nixpkgs exposes the SDK at $out/share/android-sdk.
          ANDROID_HOME = "${androidSdk}/share/android-sdk";
          ANDROID_SDK_ROOT = "${androidSdk}/share/android-sdk";
          NDK_HOME = "${androidSdk}/share/android-sdk/ndk/${ndkVersion}";
          ANDROID_NDK_HOME = "${androidSdk}/share/android-sdk/ndk/${ndkVersion}";
          ANDROID_NDK_ROOT = "${androidSdk}/share/android-sdk/ndk/${ndkVersion}";
          JAVA_HOME = "${pkgs.jdk17}/lib/openjdk";

          shellHook = ''
            echo "Orthanc dev shell ready."
            echo "  Rust targets: wasm32, *-linux-android${if isDarwin then ", *-apple-ios{,-sim}" else ""}"
            echo "  Android SDK : $ANDROID_HOME"
            echo "  Android NDK : $NDK_HOME"
            ${if isDarwin then ''
            echo ""
            echo "  iOS development requires Xcode + Command Line Tools on the host (not in nix)."
            if ! xcode-select -p > /dev/null 2>&1; then
              echo "  WARNING: xcode-select -p failed. Run 'xcode-select --install' for iOS dev."
            fi
            '' else ''
            echo "  (iOS development is macOS-only; this host can build Android only.)"
            ''}
          '';
        };
      }
    );
}
