# Packages

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

- Packages: `packaging/linux/package.sh` writes the AppImage, deb, rpm and tarball into `dist/release` (build on Ubuntu
  22.04, as CI does, so they run on its glibc and later; riscv64 is cross-built on 24.04, deb, rpm and tarball only, as the
  `linux-riscv64` job shows); `flatpak-bundle.sh` and `arch-package.sh` repackage that tarball as a Flatpak and an Arch
  package; `packaging/macos/package.sh` writes the DMG, on a Mac. After touching `packaging/`, run shellcheck on the scripts
  and actionlint on the workflow, run the Linux one through, and install the deb in a clean container. The app id is
  `io.github.ipedrax.bimcraft` everywhere (`APP_ID`, the desktop file, AppStream, the Flatpak, the bundle); the icons come
  from `packaging/icons.sh`.
- File types are registered by every package, and a new extension goes in all of them (`bimcraft_core::files` names them): a
  project is `application/x-bimcraft` (the globs `*.bimc`, `*.bimt`, `*.bimb` and the old `*.bimcraft` in the MIME XML, the
  `BIMCraft.Project` ProgId's extensions in `bimcraft.wxs`, the `io.github.ipedrax.bimcraft.project` type's in `Info.plist.in`) and a
  family is `application/x-bimcraft-family` (`*.bimf`, `BIMCraft.Family`, `io.github.ipedrax.bimcraft.family`). An old extension stays
  registered while files of it are about; the desktop file, the AppStream `<provides>` and the Flatpak install the same MIME XML.
- The APT repository is `https://bimcraft.ipedrax.com` (GitHub Pages with a custom domain: a CNAME `bimcraft` to `ipedrax.github.io` at Cloudflare,
  DNS only, not proxied, and `ipedrax.com` verified for the account, whose TXT record must stay). `release.yml`'s last two jobs build it for
  a stable tag once the release exists: `apt` takes the `.deb` files of the newest three stable releases with `gh release download`, signs with
  the key in the secret `APT_SIGNING_KEY` of the environment `apt` (tags `v*` only), and runs `packaging/linux/apt-repo.sh`
  (`apt-ftparchive`, rebuilt whole each time: no database, and the pool is what fills the site's 1 GB, hence three releases), then
  `packaging/linux/apt-repo-check.sh` installs from the result as a client does; `apt-deploy` publishes it (environment `github-pages`: `main`
  and tags `v*`). A pre-release tag is skipped, and `apt-repo.sh` refuses a `~` version. The key is ed25519, with no passphrase (CI holds
  it) and no expiry (a user's copy of the keyring doesn't update itself), fingerprint `6A9E 51B6 0E71 A9FC EA58 9203 3675 9B33 3678 A8CB`;
  its backup and revocation certificate are kept offline. If it leaks, revoke it, make another and have users fetch the new keyring again.
  Users add `bimcraft.sources` and the keyring from the site's root (`index.html` has the lines). A failed `apt` job leaves the release as it
  is; a rerun uses the workflow file of the tag's commit, so a fix to these jobs takes effect with the next tag.
- A release needs the repository secret `BIMCRAFT_KEY_SECRET` (`gh secret set BIMCRAFT_KEY_SECRET`, a long random value): the assistant's saved
  key is scrambled under it, built in when BIMCraft is compiled. `release.yml` hands it to every job that compiles and sets
  `BIMCRAFT_REQUIRE_KEY_SECRET`, so a release whose secret is missing or empty fails its tests ([the assistant](assistant.md)). Change it and
  every key a user saved with the old builds is forgotten (they type it again).
- Every package ships the licences, `NOTICE` and the interface fonts' two `OFL.txt` (the OFL asks for its text with each copy of
  the fonts, which the desktop's executable holds): `copy_docs` in `packaging/env.sh` puts them in the Linux packages (the AppImage
  too), the macOS app's `Contents/Resources` and the command line's zip, and `bimcraft.wxs` and `portable.ps1` do the same on
  Windows, under `fonts/ibm-plex-sans` and `fonts/ibm-plex-mono` beside `NOTICE`. A new package adds its copy to
  `every_package_ships_the_notice_and_the_licence_of_the_fonts` (`crates/ui-egui/src/tests/fonts.rs`). A new font goes in `NOTICE` too.
