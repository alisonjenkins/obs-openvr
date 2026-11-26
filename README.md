# obs-openvr

OpenVR capture plugin for [`obs-studio`](https://github.com/obsproject/obs-studio) on Linux. Similar to [OBS-OpenVR-Input-Plugin](https://github.com/baffler/OBS-OpenVR-Input-Plugin) on Windows.

# Usage

Add an **OpenVR Mirror Source** to a scene. It shows what one eye of the headset sees. Its settings:

* **Eye**: left or right.
* **Crop to aspect ratio**: 16:9 (the default), 4:3, 1:1, 9:16, or no crop. The crop is done on the GPU, so you don't need a `Crop/Pad` filter to hide the black edges and lens distortion.
* **Crop size (%)**: how much of the eye view to keep. 100% is the largest rectangle of that aspect that fits.
* **Move crop left/right / up/down (%)**: moves the crop. Each eye's centre is usually a little towards the nose.
* **Flip vertically**: in case the image comes out upside down on your GPU driver.
* **Copy method**: GPU (fast, the default) or CPU (slow, only for troubleshooting).

You can start OBS before SteamVR. The source connects when SteamVR starts, lets go when it quits, and connects again when it restarts.

OBS has to use its OpenGL renderer, which is the only one on Linux.

# Building & Installation

## Packages

### Nix

This repository is a flake:

```bash
nix build github:alisonjenkins/obs-openvr
```

In a NixOS or Home Manager config, add the flake's `overlays.default` and put `obs-studio-plugins.obs-openvr` in `programs.obs-studio.plugins`.

### Arch Linux

If you're on [Arch Linux](https://archlinux.org) or one of it's family of distributions that can use the [AUR](https://aur.archlinux.org), then `obs-openvr` is available as [`obs-openvr-git`](https://aur.archlinux.org/packages/obs-openvr-git/).

## Building

`obs-openvr` is built with [`cargo`](https://crates.io), and requires the following dependent libraries.

* `libobs`
* `openvr`
* `libEGL` at runtime (loaded by the mirror source to reach OpenGL)

To build, as with any `cargo` crate, just do the following.

```bash
cargo build --release
```

The output binary will then be in `target/release/libobs_openvr.so`.

## Installation

There are 3 options for installation.

1. Installation from the [AUR](https://aur.archlinux.org) package (see above)
2. After building, install as symlink to target directory (recommended for development). `./install-link.sh release`
3. After building, install directly (recommended for normal installations). `install -Dm 0644 -T target/release/libobs_openvr.so ~/.config/obs-studio/plugins/obs-openvr/bin/64bit/libobs-openvr.so`
