{
  description = "OBS source that captures the SteamVR (OpenVR) headset mirror view on Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        obs-openvr = pkgs.callPackage ./nix/package.nix { };
        default = obs-openvr;
      });

      overlays.default = final: _prev: {
        obs-studio-plugins = _prev.obs-studio-plugins // {
          obs-openvr = final.callPackage ./nix/package.nix { };
        };
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.default ];
          packages = [ pkgs.rust-analyzer pkgs.clippy pkgs.rustfmt ];
        };
      });
    };
}
