# Releases, signing and the installer

Every push to `main` runs `.github/workflows/release.yml` and publishes to the public `client` bucket.

## Jobs

| Job       | What it does                                                                                                                  | Secrets              |
| --------- | ----------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| `build`   | Builds Windows (cross-compiled) and Linux, with `VOLCREN_VERSION` and `VOLCREN_BUILD` (the commit's Unix time).                | None                 |
| `package` | The Linux archive: a `volcren/` folder with `loader`, `injector`, `injector.exe`, `agent.dll`.                                 | None                 |
| `msi`     | Builds the MSI with WiX on `windows-latest` and runs `installer/test-msi.ps1`. Also runs on pull requests.                     | None                 |
| `sign`    | Writes the manifest with each package's sha256 and signs it. Push only.                                                        | `UPDATE_SIGNING_KEY` |
| `publish` | Checks `latest.signed.json` against the keys in `update.rs` before anything is public, uploads the packages, then `latest.signed.json`, then `latest.json`, and checks both published manifests are the uploaded bytes. Push only; the key is only in the upload step's environment.    | `SUPABASE_SERVICE_KEY` |
| `release` | Tags the commit and makes a GitHub release with the same packages. Push only.                                                  | Job token            |

## Manifests

- `latest.signed.json` is what the loader reads: `{ "manifest": "<the manifest as a string>", "signature": "<minisign text>" }`. One object, so a loader can't read a new signature with an old manifest.
- `latest.json` is the same manifest unsigned, for the webapp's install page and older loaders. Nothing installs from it.
- The signature covers the manifest's exact bytes, and the manifest carries each package's sha256, so one signature vouches for everything. Its `build` orders releases, so an older signed manifest, even from the same day, is never offered.

## Signing keys

The loader embeds two minisign public keys (`PUBLIC_KEYS` in `src/loader/src/update.rs`) and accepts either. Verification can't be turned off. Signing uses `rsign2`, which is minisign-compatible.

- The current private key is the `UPDATE_SIGNING_KEY` secret on the `release-signing` GitHub Environment, which should only allow `main`. It is kept away from `SUPABASE_SERVICE_KEY`, so a leaked Supabase key can't ship code.
- The next private key stays offline, never on a runner.
- Private keys never go in code, docs or `.env*` files.

To rotate, once the current key is retired or leaked:

1. Set `UPDATE_SIGNING_KEY` to the next private key. Releases keep verifying, because installed loaders already trust it.
2. Generate a new next key: `rsign generate -W -p next.pub -s next.key`.
3. In `PUBLIC_KEYS`, put the old next key first and the new one second, then release.
4. Re-sign `src/loader/src/update/signed_fixture.json`, the test fixture proving the first key accepts what the tool signs. Its `build` of 0 means it can never be offered as an update.

A loader that trusts neither key can only be updated by a manual install.

## The MSI

`installer/volcren.wxs`, one dual-purpose package (`Scope="perUserOrMachine"`).

- **Modes.** Per-user goes to `%LOCALAPPDATA%\Programs\VOLCREN` with no UAC prompt, per-machine to `%ProgramFiles%\VOLCREN`. An unattended install with no properties is per-user.
- **Dialog.** Our own short set: Welcome, the stock `InstallScopeDlg`, then Install. The stock `WixUI_Advanced` hides the choice behind a licence page, and there is no licence page today. A standard user isn't offered per-machine.
- **Upgrades don't cross modes.** Windows Installer won't upgrade across them, so a second install in the other mode leaves two copies; uninstall first to change mode. The loader's update passes the existing mode and folder.
- **`UpgradeCode`** is fixed forever and matches `UPGRADE_CODE` in `src/loader/src/update/msi.rs`.
- **Version.** Windows Installer allows `255.255.65535` and ignores a fourth field, so `2026.11.05-abc1234` becomes `<year - 2000>.<month>.<minute of the month, UTC>`. `AllowSameVersionUpgrades` covers re-running a commit.
- **`ARPINSTALLLOCATION`** is how the loader finds its own product, and so whether it was installed per-user, per-machine or not by the MSI at all.
- **Not code-signed.** Windows warns on first run, per-machine UAC shows an unknown publisher, and Smart App Control blocks it until it's turned off. Minisign protects updates; it says nothing to Windows.
- **WiX** is pinned to 5.0.2 in the workflow, under MS-RL with no fee. Don't move to v6 or later without deciding on its Open Source Maintenance Fee, which applies to revenue-generating use (v7: above US$10,000 a year). v5 gets no new fixes from upstream, which matters little here: it only runs in CI, and the MSI has no custom action code.

## What CI tests and what it can't

`installer/test-msi.ps1` runs the real binaries on real Windows, in both modes: install, update through `--finish-update` as the loader starts it, uninstall. It asks Windows Installer for the mode, version and location, the way the loader does, and checks the files. A failure stops the release before anything is signed.

It can't cover the dialog, the UAC prompt or injecting from an installed copy. Check those on a Windows machine after changing them.
