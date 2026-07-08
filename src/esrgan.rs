use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result};

use crate::config::cache_dir;

const ESRGAN_MACOS_URL: &str = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-macos.zip";
const ESRGAN_LINUX_URL: &str = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-ubuntu.zip";
const ESRGAN_MODEL: &str = "realesrgan-x4plus";

fn esrgan_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("artfetch")
        .join("realesrgan")
}

fn esrgan_bin() -> PathBuf {
    esrgan_dir().join("realesrgan-ncnn-vulkan")
}

fn esrgan_models_dir() -> PathBuf {
    esrgan_dir().join("models")
}

fn esrgan_model_installed() -> bool {
    let models = esrgan_models_dir();
    models.join(format!("{}.param", ESRGAN_MODEL)).is_file()
        && models.join(format!("{}.bin", ESRGAN_MODEL)).is_file()
}

pub fn esrgan_installed() -> bool {
    esrgan_bin().is_file() && esrgan_model_installed()
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

pub struct UpscaleReport {
    pub success: bool,
    pub status: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub log_path: Option<PathBuf>,
}

impl UpscaleReport {
    pub fn failure_summary(&self) -> String {
        if let Some(line) = self.stderr.lines().find(|line| !line.trim().is_empty()) {
            return line.trim().to_string();
        }
        if let Some(line) = self.stdout.lines().find(|line| !line.trim().is_empty()) {
            return line.trim().to_string();
        }
        match self.status {
            Some(code) => format!("Real-ESRGAN exited with code {}", code),
            None if self.signal.is_some() => {
                format!("Real-ESRGAN was killed by signal {}", self.signal.unwrap())
            }
            None => "Real-ESRGAN failed to start".into(),
        }
    }
}

#[cfg(unix)]
fn exit_signal(status: std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;

    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: std::process::ExitStatus) -> Option<i32> {
    None
}

fn debug_enabled() -> bool {
    env::var("ARTFETCH_UPSCALE_DEBUG")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn write_debug_log(
    input: &Path,
    output: &Path,
    bin: &Path,
    models: &Path,
    report: &UpscaleReport,
) -> Option<PathBuf> {
    if !debug_enabled() {
        return None;
    }

    let log_path = cache_dir().join("upscale-debug.log");
    let _ = fs::create_dir_all(log_path.parent()?);

    let mut body = String::new();
    let _ = writeln!(body, "input: {}", input.display());
    let _ = writeln!(body, "output: {}", output.display());
    let _ = writeln!(body, "binary: {}", bin.display());
    let _ = writeln!(body, "models: {}", models.display());
    let _ = writeln!(body, "models_exists: {}", models.is_dir());
    let _ = writeln!(body, "status: {:?}", report.status);
    let _ = writeln!(body, "signal: {:?}", report.signal);
    let _ = writeln!(body, "output_exists: {}", output.exists());
    let _ = writeln!(body, "\nstdout:\n{}", report.stdout);
    let _ = writeln!(body, "\nstderr:\n{}", report.stderr);

    fs::write(&log_path, body).ok()?;
    Some(log_path)
}

pub fn upscale_image(input: &Path, output: &Path) -> UpscaleReport {
    let bin = esrgan_bin();
    let models = esrgan_models_dir();

    let mut command = Command::new(&bin);
    if !esrgan_model_installed() {
        let mut report = UpscaleReport {
            success: false,
            status: None,
            signal: None,
            stdout: String::new(),
            stderr: format!("Real-ESRGAN model files missing in {}", models.display()),
            log_path: None,
        };
        report.log_path = write_debug_log(input, output, &bin, &models, &report);
        return report;
    }

    command
        .current_dir(esrgan_dir())
        .args(["-i"])
        .arg(input)
        .args(["-o"])
        .arg(output)
        .args(["-s", "2", "-n", ESRGAN_MODEL]);

    if models.is_dir() {
        command.args(["-m"]).arg(&models);
    }

    let mut report = match command.output() {
        Ok(output_result) => UpscaleReport {
            success: output_result.status.success() && output.exists(),
            status: output_result.status.code(),
            signal: exit_signal(output_result.status),
            stdout: String::from_utf8_lossy(&output_result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output_result.stderr).into_owned(),
            log_path: None,
        },
        Err(error) => UpscaleReport {
            success: false,
            status: None,
            signal: None,
            stdout: String::new(),
            stderr: error.to_string(),
            log_path: None,
        },
    };

    report.log_path = write_debug_log(input, output, &bin, &models, &report);
    report
}
