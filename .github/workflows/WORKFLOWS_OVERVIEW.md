# GitHub Actions Workflows Overview

This document provides a quick overview of all available CI/CD workflows in this repository.

**Note:** All workflows in this repository use **manual triggers only** (`workflow_dispatch`). There are no automatic triggers from push or pull request events.

## Workflow Files

### 1. **build-devtest.yml** - DevTest Builds
**Purpose:** Fast builds for development and testing

**Key Features:**
- Signing OFF by default (faster builds)
- Per-platform signing inputs: `sign-macos` (Apple), `sign-windows` (DigiCert KeyLocker)
- All platforms in parallel
- 14-day artifact retention

**Triggers:**
- Manual dispatch only

**Use When:**
- Regular development work
- Testing features
- Need fast feedback

---

### 2. **build-macos.yml** - macOS Standalone Builds
**Purpose:** Build and test specifically for Apple Silicon (M1/M2/M3)

**Key Features:**
- Apple Developer Certificate signing (optional)
- Notarization with Apple ID
- Signature verification
- macOS-focused optimizations

**Triggers:**
- Manual dispatch only

**Use When:**
- macOS-specific development
- Testing Metal GPU acceleration
- Verifying macOS-specific features

**Outputs:**
- `.dmg` installer
- `.app` bundle

---

### 3. **build-windows.yml** - Windows Standalone Builds
**Purpose:** Build and test specifically for Windows x64

**Key Features:**
- DigiCert KeyLocker signing (cloud HSM)
- Signs both MSI and NSIS installers
- Signature verification with PowerShell
- MSI installer validation

**Triggers:**
- Manual dispatch only

**Use When:**
- Windows-specific development
- Testing Vulkan GPU acceleration
- Verifying Windows-specific features

**Outputs:**
- `.msi` installer
- `.exe` NSIS installer

---

### 4. **build-linux.yml** - Linux Standalone Builds
**Purpose:** Build and test for Linux distributions

**Key Features:**
- Support for Ubuntu 22.04 and 24.04
- Multiple bundle formats (DEB, AppImage, RPM)
- AppImage compatibility fixes
- Package verification

**Triggers:**
- Manual dispatch only

**Use When:**
- Linux-specific development
- Testing Vulkan GPU acceleration
- Verifying package formats

**Outputs:**
- `.deb` package (Ubuntu/Debian)
- `.AppImage` portable
- `.rpm` package (Fedora/RHEL)

---

### 5. **build-test.yml** - Multi-Platform Test Builds
**Purpose:** Test builds across all platforms

**Key Features:**
- **Unsigned** builds; `sign-macos` / `sign-windows` default to false, so the run needs no Apple or DigiCert secrets
- All platforms in parallel
- Uses reusable `build.yml` workflow
- 30-day artifact retention
- Artifacts prefixed with `matchwise-test-`

**Triggers:**
- Manual dispatch only

**Use When:**
- Pre-release testing
- Verifying that all platforms still bundle
- Testing across all platforms simultaneously

---

### 6. **build.yml** - Reusable Build Workflow
**Purpose:** Shared workflow used by other workflows

**Key Features:**
- Reusable workflow (called by others)
- Highly configurable inputs
- Used by `build-test.yml` and `release.yml`

**Not directly triggered** - used as a building block

---

### 7. **release.yml** - Production Release
**Purpose:** Create official releases

**Key Features:**
- macOS ships **ad-hoc signed** (`signingIdentity: "-"` in `tauri.conf.json`); a Developer ID certificate and notarization are not wired up yet
- Windows ships **unsigned** until a DigiCert KeyLocker certificate exists; set `sign-windows: true` then
- Creates GitHub Release (draft)
- Version tags from `tauri.conf.json`
- Uploads release assets
- **macOS and Windows only** (Linux excluded from production releases)
- **Auto-increment versioning**: If tag exists, auto-increments (e.g., `0.1.1` -> `0.1.1.1` -> `0.1.1.2`, up to `.100`)

**Triggers:**
- Manual dispatch only

**Use When:**
- Ready to publish a new version
- Creating official release artifacts

**Outputs:**
- GitHub Release (draft)
- macOS: DMG installer, app bundle
- Windows: MSI installer, NSIS installer
- Release notes auto-generated

**Version Behavior:**
- If `v0.1.1` tag doesn't exist: creates `v0.1.1`
- If `v0.1.1` exists: creates `v0.1.1.1`
- If `v0.1.1.1` exists: creates `v0.1.1.2`
- Maximum: `v0.1.1.100` (then update `tauri.conf.json`)

**Note:** Linux builds are not included in releases. Use `build-linux.yml` for Linux testing.

---

### 8. **pr-main-check.yml** - Validation Check
**Purpose:** Quick validation of version and configuration

**Key Features:**
- No builds triggered
- Validates version format
- Shows current branch info
- Provides next steps guidance

**Triggers:**
- Manual dispatch only

**Use When:**
- Quick configuration check
- Before running full builds

---

## How to Run Workflows

1. **Go to Actions tab** in GitHub repository
2. **Select workflow** from left sidebar
3. **Click "Run workflow"** button
4. **Select branch** to run against
5. **Configure options** (build type, signing, etc.)
6. **Click "Run workflow"** to start
7. **Monitor progress** in the Actions tab

---

## Quick Decision Guide

### "I'm developing a new feature..."
- **Use `build-devtest.yml`** (manual dispatch)
- Fast builds, no signing by default
- Enable signing checkbox if needed

### "I need to test macOS-specific code..."
- **Use `build-macos.yml`** (manual dispatch)
- Focus on macOS
- Optional signing

### "I need to test Windows-specific code..."
- **Use `build-windows.yml`** (manual dispatch)
- Focus on Windows
- Optional signing

### "I need to test Linux packages..."
- **Use `build-linux.yml`** (manual dispatch)
- Choose Ubuntu version
- Choose bundle types

### "I need to check every platform still bundles..."
- **Use `build-test.yml`** (manual dispatch)
- All platforms, unsigned
- No signing secrets required

### "I'm ready to release..."
- **Use `release.yml`** (manual dispatch)
- Creates GitHub Release
- macOS + Windows bundles (macOS ad-hoc signed, Windows unsigned)
- Production-ready once a signing certificate is in place

---

## Workflow Dependencies

```
build.yml (reusable)
    |-- build-test.yml (calls build.yml)
    |-- release.yml (calls build.yml)

Standalone (don't use build.yml):
    |-- build-macos.yml
    |-- build-windows.yml
    |-- build-linux.yml
    |-- build-devtest.yml
    |-- pr-main-check.yml (validation only)
```

---

## Comparison Matrix

| Workflow | Platforms | Default Signing | Speed | Retention | Use Case |
|----------|-----------|----------------|-------|-----------|----------|
| `build-devtest.yml` | All | OFF | Fast | 14 days | Development |
| `build-macos.yml` | macOS | Optional | Medium | 30 days | macOS dev |
| `build-windows.yml` | Windows | Optional | Medium | 30 days | Windows dev |
| `build-linux.yml` | Linux | Optional | Medium | 30 days | Linux dev |
| `build-test.yml` | All | OFF | Slow | 30 days | Pre-release smoke test |
| `release.yml` | macOS + Windows | OFF (no cert yet) | Slow | Permanent | Release |

---

## Artifact Naming Convention

```
matchwise-{workflow}-{platform}-{target}-{version}
```

**Examples:**
- `matchwise-devtest-macOS-aarch64-apple-darwin-0.1.3`
- `matchwise-test-windows-x86_64-pc-windows-msvc-0.1.3`
- `matchwise-macos-aarch64-release-0.1.3`

---

## Required Secrets

Only the **signed** paths need secrets; unsigned builds (the default) need none.

### macOS Signing (`sign-macos: true`)
- `APPLE_CERTIFICATE` - Developer ID certificate (base64)
- `APPLE_CERTIFICATE_PASSWORD` - Certificate password
- `APPLE_ID` - Apple ID email
- `APPLE_PASSWORD` - App-specific password
- `APPLE_TEAM_ID` - Team ID
- `KEYCHAIN_PASSWORD` - Temporary keychain password

### Windows Signing (`sign-windows: true`)
- `SM_HOST` - DigiCert host URL
- `SM_API_KEY` - API key
- `SM_CLIENT_CERT_FILE_B64` - Client cert (base64)
- `SM_CLIENT_CERT_PASSWORD` - Client cert password
- `SM_CODE_SIGNING_CERT_SHA1_HASH` - Certificate hash

### Application Configuration

---

## Performance Tips

1. **Use devtest workflow** for routine development (fastest)
2. **Enable signing** (`sign-macos` / `sign-windows`) only when necessary (adds 10-15 minutes)
3. **Test specific platforms** when working on platform-specific code
4. **Run full builds** (`build-test.yml`) before releases
5. **Cache is enabled** - subsequent builds are faster

---

## Troubleshooting

### Build fails with version error (Windows MSI)
- Ensure version in `tauri.conf.json` doesn't contain non-numeric pre-release identifiers
- Use `0.1.3` not `0.1.2-pro-trial`

### Signing fails
- Verify all required secrets are configured
- Check secret expiration dates
- Review workflow logs for specific errors

### Artifacts not available
- Check build succeeded completely
- Artifacts expire based on retention period
- Ensure `upload-artifacts` is enabled

### Workflow not appearing in Actions
- Verify YAML syntax is valid
- Check file is in `.github/workflows/` directory
- Ensure file extension is `.yml` or `.yaml`

---

## Support

For issues with workflows:
1. Check workflow logs in Actions tab
2. Review this documentation
3. Check `README_DEVTEST.md` for devtest-specific help
4. Check `ACCELERATION_GUIDE.md` for GPU/performance info
