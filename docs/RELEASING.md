# Releasing and going public

## One-time setup (owner)

1. **Android signing key (S12)** — create it on your own computer, never in the repository:
   `keytool -genkeypair -v -keystore release.jks -keyalg RSA -keysize 4096 -validity 10000 -alias rdpv`
   Keep `release.jks` and its passwords offline (e.g. a password manager plus an
   offline backup). Losing it means future versions cannot update installed copies.
2. **Repository secrets** (*Settings → Secrets and variables → Actions*):
   `ANDROID_KEYSTORE_BASE64` (`base64 -w0 release.jks`), `ANDROID_KEYSTORE_PASSWORD`,
   `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
   Optional: `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` for an Authenticode
   code-signing certificate. Without them the Windows files are built **unsigned**.
3. Protect tags `v*` (*Settings → Rules*) so only you can create release tags.

## Each release

1. Update the version in **both** `app/pubspec.yaml` (`version: X.Y.Z+N`, raise `N` every
   release for Android) and `Cargo.toml` (`version = "X.Y.Z"`). Note changes in
   `docs/PROGRESS.md`.
2. Make sure CI and the Security workflow are green on `main`.
3. `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. The **Release** workflow builds and checks every package:
   - versions match the tag; no local paths in any binary (SA-9);
   - `.deb` installed, launched and removed on a clean runner;
   - MSI installed per user, checked and uninstalled silently;
   - APK signature verified, never the debug key;
   - SBOM (CycloneDX) and `SHA256SUMS` generated.
   It then creates a **draft** release.
5. Review the draft: download a few files, check `sha256sum -c SHA256SUMS`, try the
   installers (see the manual checklist below), then **Publish**.

### Manual checklist per release (needs real devices)

- [ ] Windows: install MSI → Start menu and desktop shortcut work → connect → save a
      password → install the next version over it (upgrade keeps connections) →
      uninstall → Credential Manager has no entries for `io.github.securerdpviewer`.
- [ ] Windows portable ZIP from a USB drive: `data` folder created next to the app;
      the Save-password warning appears.
- [ ] Ubuntu: `.deb` install, app-menu entry, connect, *Remove all saved passwords*, remove.
- [ ] AppImage on another distribution; with a `.data` folder it keeps data there.
- [ ] Android: install APK; signing certificate matches the release notes; fingerprint
      prompt for saved passwords; screenshots blocked; leave the app 5 minutes →
      session ends with Reconnect offered.

## Going public (once, SA-9 and SA-5)

Do **not** make the repository public until every item is done:

- [ ] Independent security review (SA-5) completed and its findings fixed.
- [ ] The SF-2 report has been sent to Devolutions privately, and they have fixed it or
      agreed to disclosure.
- [ ] Full-history secret scan is clean (Security workflow → *Secret scan*), and
      `gitleaks git` run locally over a fresh clone is clean too.
- [ ] Every commit uses the noreply identity: `git log --format='%ae' | sort -u`.
- [ ] No personal data in issues, pull requests, workflow logs or release notes.
- [ ] Binaries from the release workflow passed the local-path check.
- [ ] `README.md`, `SECURITY.md` and `docs/` are current; private vulnerability
      reporting is enabled (*Settings → Code security*).
- [ ] Branch protection on `main` (pull requests, required CI checks).

Then *Settings → General → Danger zone → Change visibility → Public*.
