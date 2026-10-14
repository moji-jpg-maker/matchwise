# M9 status: Windows distribution

An installer a stranger can run on a clean Windows machine. This is the first milestone where the work cannot be
finished from Linux: the `.ps1` scripts cannot be executed in this sandbox (no `pwsh`/`powershell`). An unsigned
`workflow_dispatch` of `build-windows.yml` has since gone green (run `38528056988`, ~32 min) and produced both
installers, so "does it build on Windows" is answered. What a run cannot answer is whether the installer
*installs*: no one has run it on a clean machine, and the signing path still has no certificate. What follows
separates what a run has demonstrated from what is still a claim.

## What was found (verified by reading, not by running)
- **The signing script exists.** `bundle.windows.signCommand` in `frontend/src-tauri/tauri.conf.json` is
  `powershell ... -File scripts/sign-windows.ps1 -FilePath %1`, and the path is relative to `frontend/src-tauri/`.
  The file is there: `frontend/src-tauri/scripts/sign-windows.ps1`. It exits 0 when `DIGICERT_KEYPAIR_ALIAS` is unset,
  so an unsigned build is clean; with the alias set it runs `smctl sign` and then asserts `Get-AuthenticodeSignature`
  reports `Valid`. The earlier PLAN text ("does not exist in `frontend/scripts/`") was a path mistake and is corrected.
- **The sidecars are already produced on Windows.** `build.yml`, `build-windows.yml` and `build-devtest.yml` build
  `llama-helper` on Windows and copy it to `binaries/llama-helper-x86_64-pc-windows-msvc.exe`. `build/ffmpeg.rs`
  downloads the per-target FFmpeg (the `windows` branch of `get_ffmpeg_url_for_target`) and `build/onnxruntime.rs`
  stages `binaries/onnxruntime`, which `tauri.windows.conf.json` bundles at the root via
  `"binaries/onnxruntime": "."`. So the Windows analogue of the Linux `binaries/` contents is downloaded/compiled by
  the build, not committed. `frontend/src-tauri/binaries/` is gitignored; nothing there is tracked.
- **The OpenSSL prerequisite is wired in and proven.** `.github/actions/windows-openssl-deps/action.yml` installs
  Strawberry Perl + NASM with `choco`, puts them on `PATH`, and throws if either is missing -- fail-fast rather than a
  confusing failure inside `openssl-src`. `build.yml` calls it for `runner.os == 'Windows'`. Run `38528056988`
  executed it in 9s: Perl 5.42.0 and NASM 3.02 both present on the runner, check passed. **M0's "untested" caveat on
  this point is retired.**
- **The model weights are not bundled.** `summary/summary_engine/model_manager.rs` resolves a per-user models dir under
  `app_data_dir` and downloads the GGUF there. So the installer does not carry a model; first use needs a download.

## What was changed
- **Signing flags are per platform.** One `sign-binaries` input guarded both the Apple steps and the DigiCert steps in
  `build.yml`, so turning signing on for one platform also ran the other platform's certificate setup, which hard-fails
  (the KeyLocker action download, then `smctl`) when those secrets are absent. Split into `sign-macos` and
  `sign-windows` in `build.yml`; the DigiCert steps now gate on `sign-windows` and the Apple env on `sign-macos`.
  `sign-binaries` is gone.
- **Standalone workflows take the same two flags.** `build-windows.yml` (`sign-build` -> `sign-windows`),
  `build-macos.yml` (`sign-build` -> `sign-macos`), and `build-devtest.yml` (split, with the signed/unsigned build steps
  keyed on either flag). `build-linux.yml` had a `sign-build` input whose only use was printing a summary line -- no
  Linux signing step anywhere -- so the dead input was dropped and the summary now says Linux bundles are unsigned.
- **Callers no longer request signing.** `release.yml` and `build-test.yml` passed `sign-binaries: true`, which would
  now have to be split and would demand DigiCert secrets; both pass `sign-macos: false` / `sign-windows: false` and
  build unsigned. This is the honest description of the current state: no certificate exists yet, and the macOS side is
  ad-hoc signed by `tauri.conf.json`'s `signingIdentity: "-"`, so it never used the Apple secrets in the first place.
- **Rebranded the pipeline.** `asset-prefix` default `meetily` -> `matchwise`; artifact names
  `meetily-{windows,macos,linux,devtest}-*` -> `matchwise-*`; `release.yml`'s draft release title
  `Meetily v...` -> `Matchwise v...`. The macOS and Windows artifact uploads in `build.yml` both used
  `${{ inputs.asset-prefix }}-${{ inputs.target }}`, which is unique only because the two triples differ today; the
  platform segment was added so a future shared triple cannot collide.
- **The `resources` merge trap was fixed, not just documented.** `tauri.conf.json`'s `resources` became a map
  (`{"templates/*.json": "templates"}`) and the redundant `"templates": "templates"` key was dropped from
  `tauri.windows.conf.json`. Before this, the base array was discarded outright on Windows and only the overlay's own
  re-listing kept the templates shipping -- any future base-array addition would have vanished on Windows with no
  error. Map form on both sides makes the merge compose. See "Things to know" for the mechanism.
- **The signing path fails fast when it has no secrets.** A `Require DigiCert secrets` step now runs first in the
  Windows signing sequence of `build-windows.yml`, `build.yml` and `build-devtest.yml`, checks the five `SM_*` secrets
  for emptiness and stops with a named list before the KeyLocker action is downloaded. Previously a dispatch with
  `sign-windows` on and no secrets configured died at the cert-sync step with `403 Invalid credentials`, which reads
  like a build break rather than a missing prerequisite -- run `38526550067` failed exactly that way.
- **Docs.** `WORKFLOWS_OVERVIEW.md` matched the new inputs, naming and the honest signing status;
  `ACCELERATION_GUIDE.md` and `frontend/src-tauri/templates/README.md` lost the last upstream product-name references
  (the templates README now names the `Matchwise` directory the loader actually uses -- `dirs::data_dir()` +
  `"Matchwise"`, not the bundle identifier).

## What a Windows run demonstrated
Run `38528056988` (`workflow_dispatch` of `build-windows.yml` on `main`, `headSha a262ea9`, `sign-windows=false`,
~32 min, all steps green):
- **Both installers built.** `matchwise_0.4.1_x64_en-US.msi` (68M) and `matchwise_0.4.1_x64-setup.exe` (43M) under
  `target/x86_64-pc-windows-msvc/release/bundle/`. The summary table reads `Signed: ❌ No`, as intended.
- **The resources fix shipped, contents included.** The MSI's cab file table lists all six template JSONs next to
  `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `llama-helper.exe` and `ffmpeg.exe`, each with an
  MSI-recorded size equal to the on-disk file **plus its newline count** (the CRLF checkout: 1009->1036,
  1086->1125, 1483->1527, 1582->1625, 1852->1903, 2581->2644 -- six exact matches). An array-shaped or discarded
  `resources` key could not produce that table. The merge fix is confirmed end-to-end, not just by reading.
- **The rest of the pipeline runs.** Vulkan SDK 1.4.309.0 configured; `llama-helper` sidecar compiled; ONNX Runtime
  staged (`Bundled verified ONNX Runtime at ...\frontend\src-tauri\binaries\onnxruntime`); Tauri bundle 22m.
- `SignTool Error: No signature found` plus `Skipping signing - DIGICERT_KEYPAIR_ALIAS not set` is the expected
  unsigned path -- `sign-windows.ps1` exits 0 by design when the alias is unset.

## Still open (needs a Windows machine, or a certificate, or both)
- **No certificate, so no signed installer.** No `SM_*` secret is configured. `sign-windows.ps1` is correct as far as
  reading goes but has never signed anything: no `.ps1` can be executed or linted in this sandbox. The new preflight
  step means a `sign-windows` dispatch now fails immediately with the missing secret names instead of a 403. The
  OV-versus-EV choice and whether to submit the build for SmartScreen reputation are still decisions, not work.
- **The installer has still never been installed.** Building and installing are different claims; everything below
  needs a clean Windows VM.
- **The QA checklist below has not been run at all.** It is a checklist, not a result.

## M9 QA checklist (to run on a clean Windows VM)
- [ ] Install from the NSIS `.exe`; confirm the SmartScreen/Defender prompt and what it says about the publisher.
- [ ] Install from the MSI; confirm the per-user/per-machine choice and that it lands where expected.
- [ ] Shortcut and Start-menu entry appear and launch the app.
- [ ] First run creates the encrypted database (no plaintext file in `%APPDATA%`); the key is in Windows Credential
      Manager, not in a file.
- [ ] AI gate with no model present: the app asks rather than fails, and says a download is needed.
- [ ] Bulk intake (M8) accepts pasted and loaded entries.
- [ ] A ranked run (M3) returns matches.
- [ ] Install an older build, then upgrade over it: data survives, no second database, key still resolves.
- [ ] Uninstall: app files gone, and **no key left behind** in Credential Manager.
- [ ] Record what a genuinely fresh machine asks the user for before the first screen appears.

## Things to know when extending
- `signCommand` paths are relative to `frontend/src-tauri/`. A script reference that looks missing is usually a
  path-resolution mistake, not a missing file.
- **`resources` must stay a map, on every platform.** `tauri.conf.json` is merged with `json_patch::merge` (RFC 7386,
  `tauri-utils` `config/parse.rs` -> `json-patch` `lib.rs`), and that function replaces a non-object target wholesale
  when the patch value is an object. So a base `resources` **array** with a platform overlay that uses a **map** loses
  the base entry on that platform: the array is discarded, not concatenated. This was the case until it was fixed --
  the Windows overlay happened to re-list templates, masking it, and anything added to the base array would have
  silently not shipped on Windows. Both files now use the map form
  (`{"templates/*.json": "templates"}` in the base, adding `"binaries/onnxruntime": "."` on Windows), so the patch
  recurses key-by-key and the entries compose. Keep new resources in map form.
- The signing flags are booleans on `workflow_dispatch`; `release.yml` calls the reusable `build.yml` with inputs, so
  `sign-binaries` references would fail fast at the workflow level. Grep for the old name before adding a caller.
- `frontend/src-tauri/binaries/` is gitignored on purpose -- binaries are fetched or compiled at build time. Do not
  "fix" a Windows build by committing an `.exe` there.
