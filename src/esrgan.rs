use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result};

const ESRGAN_MACOS_URL: &str = "https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan/releases/download/v0.2.0/realesrgan-ncnn-vulkan-v0.2.0-macos.zip";
const ESRGAN_LINUX_URL: &str = "https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan/releases/download/v0.2.0/realesrgan-ncnn-vulkan-v0.2.0-ubuntu.zip";

fn esrgan_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("artfetch")
        .join("realesrgan")
}

fn esrgan_bin() -> PathBuf {
    esrgan_dir().join("realesrgan-ncnn-vulkan")
}

pub fn esrgan_installed() -> bool {
    esrgan_bin().is_file()
}

pub async fn install_esrgan(client: &reqwest::Client) -> Result<()> {
    let url = if env::consts::OS == "linux" {
        ESRGAN_LINUX_URL
    } else {
        ESRGAN_MACOS_URL
    };

    let bytes = client
        .get(url)
        .send()
        .await
        .context("Failed to download Real-ESRGAN")?
        .bytes()
        .await?;

    let dir = esrgan_dir();
    let _ = fs::create_dir_all(&dir);
    let zip_path = dir.join("_download.zip");
    fs::write(&zip_path, &bytes)?;

    let status = Command::new("unzip")
        .args(["-o"])
        .arg(&zip_path)
        .arg("-d")
        .arg(&dir)
        .output()
        .context("unzip not found")?;

    let _ = fs::remove_file(&zip_path);

    if !status.status.success() {
        anyhow::bail!("unzip failed");
    }

    if !esrgan_bin().exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let nested = entry.path().join("realesrgan-ncnn-vulkan");
                if nested.exists() {
                    for inner in fs::read_dir(entry.path())? {
                        let inner = inner?;
                        let dest = dir.join(inner.file_name());
                        let _ = fs::rename(inner.path(), &dest);
                    }
                    let _ = fs::remove_dir_all(entry.path());
                    break;
                }
            }
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = esrgan_bin();
        if bin.exists() {
            let mut perms = fs::metadata(&bin)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bin, perms)?;
        }
    }

    if esrgan_installed() {
        Ok(())
    } else {
        anyhow::bail!(
            "Installation succeeded but binary not found at {:?}",
            esrgan_bin()
        )
    }
}

pub fn upscale_image(input: &Path, output: &Path) -> bool {
    let bin = esrgan_bin();
    let models = esrgan_dir().join("models");
    Command::new(&bin)
        .args(["-i"])
        .arg(input)
        .args(["-o"])
        .arg(output)
        .args(["-s", "2", "-n", "realesrgan-x4plus"])
        .args(["-m"])
        .arg(&models)
        .output()
        .map(|r| r.status.success() && output.exists())
        .unwrap_or(false)
}
