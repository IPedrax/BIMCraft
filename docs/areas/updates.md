# Updates and installs

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Updates (`crates/update`, drawn by `crates/ui-egui/src/updates.rs`). At startup the desktop asks `api.github.com` for the latest
release (`LATEST_URL`: never a draft or a pre-release) on a thread of its own, and compares its tag, `vMAJOR.MINOR.PATCH`, with the
running version as semver (`tag_version`, `is_newer`). Errors are quiet at startup and said in the status box when the user
asked (File > Check for Updates Now). A release whose installer isn't sound is still told of, with a Download button. The user's
choices are `update.json` beside the favorites (`Prefs`, `prefs::beside` the path `favorites::default_path` finds: `%APPDATA%\BIMCraft`,
`~/Library/Application Support/BIMCraft`, `$XDG_CONFIG_HOME/bimcraft` else `~/.config/bimcraft`, or the folder of the file
`BIMCRAFT_FAVORITES` names; the desktop's `main` hands it to `Updater::desktop`, because this crate doesn't depend on the core), never a project: whether to
check at startup, and the version they skipped (it and anything older are quiet at startup, a newer one is offered). The
environment variable `BIMCRAFT_NO_UPDATE_CHECK`, set to anything but empty, leaves the app with no updater at all. The release workflow must keep naming its
assets as `Target::asset_name` does (`bimcraft-<version>-windows-x64.msi` and the other two) and publish them with `gh release
create`, which is what makes GitHub give each a digest; rename an asset and updates quietly stop being installable. Which file of a release updates an install is the
`Target`: the Windows MSI, a Linux AppImage per `Arch` (`bimcraft-<version>-linux-<arch>.AppImage`) or the macOS disk image
(`bimcraft-<version>-macos-universal.dmg`); `parse_release(json, target)` takes that one and ignores the rest (the `.zsync`, the
packages, the tarball, the CLI's zip). `Install::detect()` (`detect.rs`) says which this program is and what an update would replace
(`Install { target, place }`: the AppImage file, the `.app` bundle, or the Windows program). It is none, so the release is only
told of, with a Download button, for a deb, rpm or Arch install (root's files), a tarball and a build from source. An AppImage or
a macOS app in a folder this user can't write in is told of the same way: `Install::replaceable` probes the folder, but only once
a check has found an update to offer (`fit`), never at startup, because the probe creates a file and some macOS folders (Documents,
Desktop) ask the user before an app may. A Flatpak has no network, so `Updater::desktop` makes no updater there
(`detect::in_flatpak`, `phones_home`). A release's `SHA256SUMS` (the
same workflow's, in `sha256sum`'s format), when it lists one, is asked for before the file (`cross_check`, `checksum_of`) and must
give the same sum as the API's digest; the API digest stays the one that has to exist. Restart and install is `Pending::Update`, so
unsaved changes are asked about first, and `Updater::install` then puts the update in place on a thread of its own: `State::Installing`,
then `Note::Installed` (the window closes) or `Note::InstallFailed` (the update is ready again, to try once more). While it runs a
box holds the window (`installing_box`; `guard`, `keys` and a close request all stand down while `installing()`), because the window
closes itself when the install is in and anything done meanwhile would be lost. The thread runs `Install::hand_over`, which
hashes the file again (it sat in the temp folder), replaces the install, and removes the update's `bimcraft-update-*` folder
(`detect::spent`: not the Windows one, which its copy still needs) only if that worked, so a failed install can be tried again.
On Windows, only the copy the installer put in
`%ProgramFiles%\BIMCraft` updates itself (`install::is_installed`). `install::start` copies the running program beside the
downloaded installer and starts the copy with
`--finish-update` (which `main` handles before anything else), with a pipe that ends when the app exits: the copy, which nothing
installs over, waits for that, checks that its arguments are what an update makes (`plausible`: anyone can run
`bimcraft --finish-update`, so the installer must be named for a version, in a `bimcraft-update-*` folder, and the program to
start again the one in Program Files), hashes the installer again through a handle held with `FILE_SHARE_READ` so that nothing can
swap it (should msiexec exit 1620, it lets go, checks again and runs unheld), runs `%SystemRoot%\System32\msiexec.exe /i …
/passive /norestart` by that full path (the copy's own folder is user-writable) and starts the app again with the project that
was open, whatever msiexec did. `installed_here` also refuses a copy that can write to its own folder (`writable`: that is an
administrator's, and the copy would run elevated from the temp folder). Folders `bimcraft-update-*` in the temp folder older
than a day are swept at each check, except the one an update ready to install (or being installed) is in, since the Windows copy can't remove itself.
A Linux AppImage replaces itself (`appimage.rs`), because Linux lets a running program's file be replaced: no helper copy.
`APPIMAGE` and `APPDIR` count only together with `current_exe()` lying under `APPDIR` (`detect::appimage_file`), and the file is
canonicalised, so a link updates the file it points to. The new file is copied beside the old (the same filesystem, so the last
`rename` is atomic), given the old one's mode plus execute, and run with `--version` as the user would run it and, if that fails,
once more with `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE needed), each under a time limit (`exec::run`: no input, 16 KiB of output kept,
killed at the limit). It must print `bimcraft <version>` exactly (`exec::says_version`, with `version_line`, which the desktop's
`--version` prints too so that rewording one can't strand the other; a release tagged without bumping `Cargo.toml` is refused,
rightly, since it would offer itself again) before it is renamed over the old one and started with the project (made absolute by
`Updater::install`, since macOS's `open` starts the app with `/` as its folder) and none of the old run's `APPIMAGE`, `APPDIR`,
`ARGV0` or `OWD`, and asked to unpack itself too if it had to be to run. The staged names are predictable (they hold the process
id), so they are made exclusively (`create_new`, `create_dir`): a name someone planted as a link is an error, never written through.
The macOS app (`macos.rs`) is the same idea with more steps: `hdiutil attach -readonly -nobrowse -noverify`, `ditto` of BIMCraft.app
to a hidden `.BIMCraft-update-<pid>.app` beside the installed one, `hdiutil detach -force` whatever happened, `codesign --verify
--strict --deep` (as `packaging/macos/package.sh` does), the program the copy's Info.plist names run with `--version`, then `swap`:
two renames, the old app put back if the second fails and deleted once the new one is in, and `open -n <bundle> --args <project>`.
The tools are run by absolute path (`macos::Tools`), so tests point them at shell scripts (`tests::macos`; `tests::appimage` for the
AppImage; the one test that hands a file to the real `hdiutil` is skipped on a Mac, where nobody has seen what that does). Nothing of the macOS side has run on a Mac, and the AppImage side has run only against scripts that stand in for one.
None of the Windows side runs here: its sequencing is tested with shell scripts for
msiexec and the app (`tests::unix`), and what can't be tested is UAC, the real upgrade and the Program Files check on a real install.
