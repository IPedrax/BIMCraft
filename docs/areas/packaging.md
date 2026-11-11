# Packages

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

- Packages: `packaging/linux/package.sh` writes the AppImage, deb, rpm and tarball into `dist/release` (build on Ubuntu
  22.04, as CI does, so they run on its glibc and later; riscv64 is cross-built on 24.04, deb, rpm and tarball only, as the
  `linux-riscv64` job shows); `flatpak-bundle.sh` and `arch-package.sh` repackage that tarball as a Flatpak and an Arch
  package; `packaging/macos/package.sh` writes the DMG, on a Mac. After touching `packaging/`, run shellcheck on the scripts
  and actionlint on the workflow, run the Linux one through, and install the deb in a clean container. The app id is
  `io.github.ipedrax.bimcraft` everywhere (`APP_ID`, the desktop file, AppStream, the Flatpak, the bundle); the icons come
  from `packaging/icons.sh`.
- A release needs the repository secret `BIMCRAFT_KEY_SECRET` (`gh secret set BIMCRAFT_KEY_SECRET`, a long random value): the assistant's saved
  key is scrambled under it, built in when BIMCraft is compiled. `release.yml` hands it to every job that compiles and sets
  `BIMCRAFT_REQUIRE_KEY_SECRET`, so a release whose secret is missing or empty fails its tests ([the assistant](assistant.md)). Change it and
  every key a user saved with the old builds is forgotten (they type it again).
- Every package ships the licences, `NOTICE` and the interface fonts' two `OFL.txt` (the OFL asks for its text with each copy of
  the fonts, which the desktop's executable holds): `copy_docs` in `packaging/env.sh` puts them in the Linux packages (the AppImage
  too), the macOS app's `Contents/Resources` and the command line's zip, and `bimcraft.wxs` and `portable.ps1` do the same on
  Windows, under `fonts/ibm-plex-sans` and `fonts/ibm-plex-mono` beside `NOTICE`. A new package adds its copy to
  `every_package_ships_the_notice_and_the_licence_of_the_fonts` (`crates/ui-egui/src/tests/fonts.rs`). A new font goes in `NOTICE` too.
