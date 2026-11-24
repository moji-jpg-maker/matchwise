use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process,
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

const WINDOWS_X64_TARGET: &str = "x86_64-pc-windows-msvc";
const ARCHIVE_URL: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.22.1/onnxruntime-win-x64-1.22.1.zip";
// SHA-256 published for the official Microsoft ONNX Runtime 1.22.1 Windows x64 CPU archive.
const ARCHIVE_SHA256: &str = "855276cd4be3cda14fe636c69eb038d75bf5bcd552bda1193a5d79c51f436dfe";

struct Artifact {
    archive_path: &'static str,
    output_name: &'static str,
}

const ARTIFACTS: [Artifact; 3] = [
    Artifact {
        archive_path: "onnxruntime-win-x64-1.22.1/lib/onnxruntime.dll",
        output_name: "onnxruntime.dll",
    },
    Artifact {
        archive_path: "onnxruntime-win-x64-1.22.1/lib/onnxruntime_providers_shared.dll",
        output_name: "onnxruntime_providers_shared.dll",
    },
    Artifact {
        archive_path: "onnxruntime-win-x64-1.22.1/LICENSE",
        output_name: "onnxruntime-LICENSE.txt",
    },
];

pub fn ensure_onnxruntime_runtime() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=binaries/onnxruntime");

    let target = env::var("TARGET").expect("TARGET environment variable not set");
    if target != WINDOWS_X64_TARGET {
        panic!("ONNX Runtime is bundled only for {WINDOWS_X64_TARGET}; got {target}");
    }

    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR environment variable not set");
    let destination = Path::new(&manifest_dir)
        .join("binaries")
        .join("onnxruntime");

    match verify_staged_runtime(&destination) {
        Ok(()) => {
            println!(
                "cargo:warning=Using verified bundled ONNX Runtime from {}",
                destination.display()
            );
            return;
        }
        Err(verification_error) => match fs::symlink_metadata(&destination) {
            Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                panic!(
                    "Refusing to replace linked or reparse-point ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Ok(metadata) if metadata.is_dir() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_dir_all(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime directory at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(metadata) if metadata.is_file() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_file(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime file at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(_) => {
                panic!(
                    "Refusing to replace special ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                panic!(
                    "Failed to inspect invalid ONNX Runtime stage at {}: {error}",
                    destination.display()
                );
            }
        },
    }

    fs::create_dir_all(
        destination
            .parent()
            .expect("ONNX Runtime destination has no parent"),
    )
    .expect("Failed to create ONNX Runtime binaries directory");

    let temporary_archive = env::temp_dir().join(format!(
        "matchwise-onnxruntime-{}-{}.zip",
        process::id(),
        target
    ));
    let temporary_destination =
        destination.with_file_name(format!(".onnxruntime-{}-{}", process::id(), target));
    let _ = fs::remove_file(&temporary_archive);
    let _ = fs::remove_dir_all(&temporary_destination);

    let result = stage_runtime(&temporary_archive, &temporary_destination);
    let _ = fs::remove_file(&temporary_archive);

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!("Failed to stage bundled ONNX Runtime: {error}");
    }

    fs::rename(&temporary_destination, &destination).unwrap_or_else(|error| {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!(
            "Failed to finalize bundled ONNX Runtime at {}: {error}",
            destination.display()
        );
    });

    verify_staged_runtime(&destination).unwrap_or_else(|error| {
        panic!("Bundled ONNX Runtime verification failed after staging: {error}")
    });
    println!(
        "cargo:warning=Bundled verified ONNX Runtime at {}",
        destination.display()
    );
}

fn stage_runtime(archive_path: &Path, destination: &Path) -> Result<(), String> {
    download_archive(archive_path)?;
    verify_sha256(archive_path, ARCHIVE_URL, ARCHIVE_SHA256)?;

    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;

    let archive_file = File::open(archive_path)
        .map_err(|error| format!("failed to open {}: {error}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| format!("failed to read ONNX Runtime archive: {error}"))?;

    for artifact in ARTIFACTS {
        let mut source = archive.by_name(artifact.archive_path).map_err(|error| {
            format!(
                "missing {} in ONNX Runtime archive: {error}",
                artifact.archive_path
            )
        })?;
        let output = destination.join(artifact.output_name);
        let mut file = File::create(&output)
            .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
        std::io::copy(&mut source, &mut file)
            .map_err(|error| format!("failed to extract {}: {error}", artifact.archive_path))?;
        file.flush()
            .map_err(|error| format!("failed to flush {}: {error}", output.display()))?;
        verify_nonempty_file(&output, artifact.output_name)?;
    }

    Ok(())
}

fn download_archive(destination: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| format!("failed to create download client: {error}"))?;
    let mut response = client
        .get(ARCHIVE_URL)
        .send()
        .map_err(|error| format!("failed to download ONNX Runtime from {ARCHIVE_URL}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("ONNX Runtime download failed from {ARCHIVE_URL}: {error}"))?;
    let mut file = File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    std::io::copy(&mut response, &mut file)
        .map_err(|error| format!("failed to write ONNX Runtime archive: {error}"))?;
    file.flush()
        .map_err(|error| format!("failed to flush ONNX Runtime archive: {error}"))?;
    Ok(())
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        return metadata.file_attributes() & 0x0000_0400 != 0;
    }

    #[cfg(not(windows))]
    false
}

fn verify_staged_runtime(destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| format!("failed to inspect runtime stage: {error}"))?;
    if is_link_or_reparse_point(&metadata) {
        return Err("runtime stage is a link or reparse point".to_string());
    }
    if !metadata.file_type().is_dir() {
        return Err("runtime stage is not a directory".to_string());
    }

    let entries = fs::read_dir(destination)
        .map_err(|error| format!("failed to enumerate runtime stage: {error}"))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to enumerate runtime stage entry: {error}"))?;
        let entry_metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!(
                "failed to inspect runtime stage entry {}: {error}",
                entry.path().display()
            )
        })?;
        if is_link_or_reparse_point(&entry_metadata) {
            return Err(format!(
                "runtime stage entry {} is a link or reparse point",
                entry.path().display()
            ));
        }
        if !entry_metadata.file_type().is_file() {
            return Err(format!(
                "runtime stage entry {} is not a regular file",
                entry.path().display()
            ));
        }

        let name = entry.file_name();
        if !ARTIFACTS
            .iter()
            .any(|artifact| name.as_os_str() == artifact.output_name)
        {
            return Err(format!(
                "runtime stage contains undeclared entry {}",
                entry.path().display()
            ));
        }
    }

    for artifact in ARTIFACTS {
        verify_nonempty_file(
            &destination.join(artifact.output_name),
            artifact.output_name,
        )?;
    }
    Ok(())
}

fn verify_nonempty_file(path: &Path, name: &str) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("failed to inspect {name}: {error}"))?;
    if !metadata.file_type().is_file() || is_link_or_reparse_point(&metadata) {
        return Err(format!("{name} is not a regular file"));
    }
    if metadata.len() == 0 {
        return Err(format!("{name} is empty"));
    }
    Ok(())
}

fn verify_sha256(path: &Path, name: &str, expected_sha256: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| format!("failed to open {name}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {name}: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "{name} has SHA-256 {actual_sha256}, expected {expected_sha256}"
        ));
    }

    Ok(())
}
