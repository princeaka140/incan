use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

static PREPARE_ASSETS_LOCK: Mutex<()> = Mutex::new(());

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn installer_script() -> PathBuf {
    repo_root().join("workspaces/release/install-incan-sdk.sh")
}

fn sdk_package_archive_script() -> PathBuf {
    repo_root().join("workspaces/release/sdk/package_archive.sh")
}

fn sdk_prepare_assets_script() -> PathBuf {
    repo_root().join("workspaces/release/sdk/prepare_assets.incn")
}

fn npm_prepare_package_script() -> PathBuf {
    repo_root().join("workspaces/release/npm/prepare_package.js")
}

fn npm_installer_wrapper() -> PathBuf {
    repo_root().join("workspaces/release/npm/bin/install-incan-sdk.js")
}

fn pip_prepare_package_script() -> PathBuf {
    repo_root().join("workspaces/release/pip/prepare_package.py")
}

fn pip_installer_wrapper() -> PathBuf {
    repo_root().join("workspaces/release/pip/src/incan_sdk/cli.py")
}

fn sha256_hex(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("{digest:x}"))
}

fn incan_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_incan") {
        return PathBuf::from(path);
    }
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let path = PathBuf::from(target_dir).join("debug").join("incan");
        if path.exists() {
            return path;
        }
    }
    repo_root().join("target").join("debug").join("incan")
}

fn prepare_sdk_assets(
    dist: &Path,
    generated_at: &str,
    skip_homebrew: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let _guard = PREPARE_ASSETS_LOCK.lock().map_err(|_| "prepare assets lock poisoned")?;
    let mut command = Command::new(incan_binary());
    command
        .args(["run"])
        .arg(sdk_prepare_assets_script())
        .current_dir(repo_root())
        .env("CARGO_NET_OFFLINE", "true")
        .env("INCAN_NO_BANNER", "1")
        .env("INCAN_REPO_ROOT", repo_root())
        .env("INCAN_SDK_DIST_DIR", dist)
        .env("INCAN_SDK_GENERATED_AT", generated_at)
        .env(
            "INCAN_GENERATED_CARGO_TARGET_DIR",
            repo_root().join("target/incan_generated_shared_target"),
        );
    if skip_homebrew {
        command.env("INCAN_SDK_SKIP_HOMEBREW", "1");
    }
    Ok(command.output()?)
}

fn write_fixture_archive(root: &Path) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let payload = root.join("payload");
    let bin = payload.join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("incan"), "#!/usr/bin/env sh\nprintf 'incan fixture\\n'\n")?;
    fs::write(
        bin.join("incan-lsp"),
        "#!/usr/bin/env sh\nprintf 'incan-lsp fixture\\n'\n",
    )?;

    let archive = root.join("incan-v0.4.0-test-x86_64-unknown-linux-gnu.tar.gz");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&payload)
        .arg(".")
        .status()?;
    assert!(status.success(), "tar fixture archive creation failed");

    let checksum = sha256_hex(&archive)?;
    Ok((archive, checksum))
}

fn make_executable(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn write_fixture_command(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, format!("#!/usr/bin/env sh\nprintf '{name} fixture\\n'\n"))?;
    make_executable(path)
}

fn write_fixture_sdk_commands(root: &Path) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let bin = root.join("commands");
    fs::create_dir_all(&bin)?;
    let incan = bin.join("incan");
    let incan_lsp = bin.join("incan-lsp");
    write_fixture_command(&incan, "incan")?;
    write_fixture_command(&incan_lsp, "incan-lsp")?;
    Ok((incan, incan_lsp))
}

fn package_fixture_archive(
    root: &Path,
    target: &str,
    incan: &Path,
    incan_lsp: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("bash")
        .arg(sdk_package_archive_script())
        .arg(target)
        .args(["--out-dir", root.to_str().ok_or("output path is not UTF-8")?])
        .env("INCAN_BIN", incan)
        .env("INCAN_LSP_BIN", incan_lsp)
        .current_dir(repo_root())
        .output()?;

    assert!(
        output.status.success(),
        "SDK archive packaging failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn sha256_sidecar_path(archive: &Path) -> PathBuf {
    archive.with_file_name(format!(
        "{}.sha256",
        archive.file_name().and_then(|name| name.to_str()).unwrap_or_default()
    ))
}

fn write_manifest(root: &Path, archive: &Path, checksum: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        format!(
            r#"{{
  "schema_version": 1,
  "sdk_version": "0.4.0-test",
  "release": "v0.4.0-test",
  "channel": "dev",
  "rust_toolchain": {{
    "channel": "stable",
    "min_rust": "1.92",
    "targets": ["wasm32-wasip1"],
    "policy": "fixture"
  }},
  "commands": ["incan", "incan-lsp"],
  "hosts": {{
    "x86_64-unknown-linux-gnu": {{
      "archive_url": "file://{}",
      "archive_sha256": "{}",
      "archive_format": "tar.gz",
      "commands": {{
        "incan": "bin/incan",
        "incan-lsp": "bin/incan-lsp"
      }}
    }},
    "x86_64-apple-darwin": {{
      "archive_url": "file://{}",
      "archive_sha256": "{}",
      "archive_format": "tar.gz",
      "commands": {{
        "incan": "bin/incan",
        "incan-lsp": "bin/incan-lsp"
      }}
    }},
    "aarch64-apple-darwin": {{
      "archive_url": "file://{}",
      "archive_sha256": "{}",
      "archive_format": "tar.gz",
      "commands": {{
        "incan": "bin/incan",
        "incan-lsp": "bin/incan-lsp"
      }}
    }}
  }}
}}
"#,
            archive.display(),
            checksum,
            archive.display(),
            checksum,
            archive.display(),
            checksum
        ),
    )?;
    Ok(manifest)
}

fn assert_sdk_install(incan_home: &Path, bin_dir: &Path) {
    assert!(incan_home.join("sdks/0.4.0-test/bin/incan").exists());
    assert!(incan_home.join("sdks/0.4.0-test/bin/incan-lsp").exists());
    assert!(incan_home.join("current").exists());
    assert!(bin_dir.join("incan").exists());
    assert!(bin_dir.join("incan-lsp").exists());
}

#[test]
fn sdk_archive_packager_writes_archive_checksum_and_release_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let out_dir = tmp.path().join("sdk");
    let (incan, incan_lsp) = write_fixture_sdk_commands(tmp.path())?;

    package_fixture_archive(&out_dir, "x86_64-unknown-linux-gnu", &incan, &incan_lsp)?;

    let version = fs::read_to_string(out_dir.join("sdk-version.txt"))?;
    let release = fs::read_to_string(out_dir.join("sdk-release.txt"))?;
    assert!(!version.trim().is_empty());
    assert_eq!(release.trim(), format!("v{}", version.trim()));

    let archive = out_dir.join(format!("incan-{}-x86_64-unknown-linux-gnu.tar.gz", release.trim()));
    assert!(archive.exists(), "archive was not written: {}", archive.display());
    assert_eq!(
        fs::read_to_string(sha256_sidecar_path(&archive))?.trim(),
        sha256_hex(&archive)?
    );

    let listing = Command::new("tar").arg("-tzf").arg(&archive).output()?;
    assert!(listing.status.success(), "tar listing failed");
    let listing = String::from_utf8_lossy(&listing.stdout);
    assert!(listing.contains("bin/incan"));
    assert!(listing.contains("bin/incan-lsp"));
    Ok(())
}

#[test]
fn sdk_release_assets_are_prepared_by_central_manifest_script() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let dist = tmp.path().join("sdk");
    let (incan, incan_lsp) = write_fixture_sdk_commands(tmp.path())?;

    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        package_fixture_archive(&dist, target, &incan, &incan_lsp)?;
    }

    let output = prepare_sdk_assets(&dist, "2026-06-06T00:00:00Z", false)?;

    assert!(
        output.status.success(),
        "SDK asset preparation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(dist.join("manifest.json"))?)?;
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["generated_at"], "2026-06-06T00:00:00Z");
    assert_eq!(manifest["rust_toolchain"]["targets"][0], "wasm32-wasip1");
    assert!(
        manifest["hosts"]["x86_64-unknown-linux-gnu"]["archive_url"]
            .as_str()
            .unwrap_or_default()
            .contains("/releases/download/")
    );
    assert!(dist.join("install.sh").exists());
    assert!(dist.join("sdk-manifest.schema.v1.json").exists());
    assert!(fs::read_to_string(dist.join("incan.rb"))?.contains(r#"bin.install "bin/incan""#));
    Ok(())
}

#[test]
fn sdk_release_assets_can_be_prepared_for_single_host_smoke_without_homebrew() -> Result<(), Box<dyn std::error::Error>>
{
    let tmp = tempfile::tempdir()?;
    let dist = tmp.path().join("sdk");
    let (incan, incan_lsp) = write_fixture_sdk_commands(tmp.path())?;

    package_fixture_archive(&dist, "aarch64-apple-darwin", &incan, &incan_lsp)?;

    let output = prepare_sdk_assets(&dist, "2026-06-06T00:00:00Z", true)?;

    assert!(
        output.status.success(),
        "single-host SDK asset preparation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(dist.join("manifest.json"))?)?;
    assert!(manifest["hosts"]["aarch64-apple-darwin"].is_object());
    assert!(dist.join("install.sh").exists());
    assert!(dist.join("sdk-manifest.schema.v1.json").exists());
    assert!(!dist.join("incan.rb").exists());
    Ok(())
}

#[test]
fn package_prepare_scripts_stage_versions_and_shared_installer() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let dist = tmp.path().join("sdk");
    fs::create_dir_all(&dist)?;
    fs::write(dist.join("sdk-version.txt"), "0.4.0-dev.6\n")?;

    let npm_output = Command::new("node")
        .arg(npm_prepare_package_script())
        .arg(&dist)
        .arg("--skip-pack")
        .output()?;
    assert!(
        npm_output.status.success(),
        "npm package preparation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&npm_output.stdout),
        String::from_utf8_lossy(&npm_output.stderr)
    );
    let npm_package = fs::read_to_string(dist.join("_npm-package/package.json"))?;
    assert!(npm_package.contains(r#""version": "0.4.0-dev.6""#));
    assert!(dist.join("_npm-package/vendor/install-incan-sdk.sh").exists());

    let pip_output = Command::new("python3")
        .arg(pip_prepare_package_script())
        .arg(&dist)
        .arg("--skip-build")
        .output()?;
    assert!(
        pip_output.status.success(),
        "pip package preparation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pip_output.stdout),
        String::from_utf8_lossy(&pip_output.stderr)
    );
    assert!(fs::read_to_string(dist.join("_pip-package/pyproject.toml"))?.contains(r#"version = "0.4.0.dev6""#));
    assert!(
        fs::read_to_string(dist.join("_pip-package/src/incan_sdk/__init__.py"))?
            .contains(r#"__version__ = "0.4.0.dev6""#)
    );
    assert!(
        dist.join("_pip-package/src/incan_sdk/vendor/install-incan-sdk.sh")
            .exists()
    );

    fs::write(dist.join("sdk-version.txt"), "0.4.0-rc0\n")?;
    let pip_output = Command::new("python3")
        .arg(pip_prepare_package_script())
        .arg(&dist)
        .arg("--skip-build")
        .output()?;
    assert!(
        pip_output.status.success(),
        "pip rc package preparation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pip_output.stdout),
        String::from_utf8_lossy(&pip_output.stderr)
    );
    assert!(fs::read_to_string(dist.join("_pip-package/pyproject.toml"))?.contains(r#"version = "0.4.0rc0""#));
    assert!(
        fs::read_to_string(dist.join("_pip-package/src/incan_sdk/__init__.py"))?
            .contains(r#"__version__ = "0.4.0rc0""#)
    );
    Ok(())
}

#[test]
fn sdk_installer_dry_run_selects_manifest_target_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (archive, checksum) = write_fixture_archive(tmp.path())?;
    let manifest = write_manifest(tmp.path(), &archive, &checksum)?;
    let incan_home = tmp.path().join("home");
    let bin_dir = tmp.path().join("bin");

    let output = Command::new("bash")
        .arg(installer_script())
        .args(["--manifest", manifest.to_str().ok_or("manifest path is not UTF-8")?])
        .args(["--target", "x86_64-unknown-linux-gnu"])
        .args(["--incan-home", incan_home.to_str().ok_or("home path is not UTF-8")?])
        .args(["--bin-dir", bin_dir.to_str().ok_or("bin path is not UTF-8")?])
        .arg("--dry-run")
        .output()?;

    assert!(
        output.status.success(),
        "installer dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Incan SDK 0.4.0-test"));
    assert!(stdout.contains("target:     x86_64-unknown-linux-gnu"));
    assert!(stdout.contains("Dry run only"));
    assert!(!incan_home.exists(), "dry-run must not create INCAN_HOME");
    assert!(!bin_dir.exists(), "dry-run must not create command bin directory");
    Ok(())
}

#[test]
fn sdk_installer_verifies_checksum_and_links_commands() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (archive, checksum) = write_fixture_archive(tmp.path())?;
    let manifest = write_manifest(tmp.path(), &archive, &checksum)?;
    let incan_home = tmp.path().join("home");
    let bin_dir = tmp.path().join("bin");

    let output = Command::new("bash")
        .arg(installer_script())
        .args(["--manifest", manifest.to_str().ok_or("manifest path is not UTF-8")?])
        .args(["--target", "x86_64-unknown-linux-gnu"])
        .args(["--archive", archive.to_str().ok_or("archive path is not UTF-8")?])
        .args(["--incan-home", incan_home.to_str().ok_or("home path is not UTF-8")?])
        .args(["--bin-dir", bin_dir.to_str().ok_or("bin path is not UTF-8")?])
        .output()?;

    assert!(
        output.status.success(),
        "installer failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_sdk_install(&incan_home, &bin_dir);
    Ok(())
}

#[test]
fn homebrew_formula_is_rendered_from_the_sdk_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let dist = tmp.path().join("sdk");
    let (incan, incan_lsp) = write_fixture_sdk_commands(tmp.path())?;

    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        package_fixture_archive(&dist, target, &incan, &incan_lsp)?;
    }

    let output = prepare_sdk_assets(&dist, "2026-06-06T00:00:00Z", false)?;

    assert!(
        output.status.success(),
        "formula rendering failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let checksum = fs::read_to_string(dist.join("incan-v0.4.0-rc0-x86_64-unknown-linux-gnu.tar.gz.sha256"))?
        .trim()
        .to_string();
    let formula = fs::read_to_string(dist.join("incan.rb"))?;
    assert!(formula.contains(r#"version "0.4.0-rc0""#));
    assert!(formula.contains(
        r#"url "https://github.com/dannys-code-corner/incan/releases/download/v0.4.0-rc0/incan-v0.4.0-rc0-x86_64-unknown-linux-gnu.tar.gz""#
    ));
    assert!(formula.contains(&format!(r#"sha256 "{checksum}""#)));
    assert!(formula.contains(r#"bin.install "bin/incan""#));
    assert!(formula.contains(r#"bin.install "bin/incan-lsp""#));
    Ok(())
}

#[test]
fn npm_installer_wrapper_delegates_to_shared_sdk_installer() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (archive, checksum) = write_fixture_archive(tmp.path())?;
    let manifest = write_manifest(tmp.path(), &archive, &checksum)?;
    let incan_home = tmp.path().join("npm-home");
    let bin_dir = tmp.path().join("npm-bin");

    let output = Command::new("node")
        .arg(npm_installer_wrapper())
        .args(["--manifest", manifest.to_str().ok_or("manifest path is not UTF-8")?])
        .args(["--target", "x86_64-unknown-linux-gnu"])
        .args(["--archive", archive.to_str().ok_or("archive path is not UTF-8")?])
        .args(["--incan-home", incan_home.to_str().ok_or("home path is not UTF-8")?])
        .args(["--bin-dir", bin_dir.to_str().ok_or("bin path is not UTF-8")?])
        .output()?;

    assert!(
        output.status.success(),
        "npm wrapper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_sdk_install(&incan_home, &bin_dir);
    Ok(())
}

#[test]
fn pip_installer_wrapper_delegates_to_shared_sdk_installer() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (archive, checksum) = write_fixture_archive(tmp.path())?;
    let manifest = write_manifest(tmp.path(), &archive, &checksum)?;
    let incan_home = tmp.path().join("pip-home");
    let bin_dir = tmp.path().join("pip-bin");

    let output = Command::new("python3")
        .arg(pip_installer_wrapper())
        .arg("install")
        .args(["--manifest", manifest.to_str().ok_or("manifest path is not UTF-8")?])
        .args(["--target", "x86_64-unknown-linux-gnu"])
        .args(["--archive", archive.to_str().ok_or("archive path is not UTF-8")?])
        .args(["--incan-home", incan_home.to_str().ok_or("home path is not UTF-8")?])
        .args(["--bin-dir", bin_dir.to_str().ok_or("bin path is not UTF-8")?])
        .output()?;

    assert!(
        output.status.success(),
        "pip wrapper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_sdk_install(&incan_home, &bin_dir);
    Ok(())
}
