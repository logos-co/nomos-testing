use std::{
    env,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use tokio::{process::Command, time::timeout};
use tracing::warn;

use crate::{commands::ComposeCommandError, errors::ComposeRunnerError, template::repository_root};

const IMAGE_BUILD_TIMEOUT: Duration = Duration::from_secs(600);
const DOCKER_INFO_TIMEOUT: Duration = Duration::from_secs(15);
const IMAGE_INSPECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Checks that `docker info` succeeds within a timeout.
pub async fn ensure_docker_available() -> Result<(), ComposeRunnerError> {
    let mut command = Command::new("docker");
    command
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let available = timeout(
        testing_framework_core::adjust_timeout(DOCKER_INFO_TIMEOUT),
        command.status(),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .map(|status| status.success())
    .unwrap_or(false);

    if available {
        Ok(())
    } else {
        Err(ComposeRunnerError::DockerUnavailable)
    }
}

/// Ensure the configured compose image exists, building a local one if needed.
pub async fn ensure_compose_image() -> Result<(), ComposeRunnerError> {
    let (image, platform) = crate::platform::resolve_image();
    ensure_image_present(&image, platform.as_deref()).await
}

/// Verify an image exists locally, optionally building it for the default tag.
pub async fn ensure_image_present(
    image: &str,
    platform: Option<&str>,
) -> Result<(), ComposeRunnerError> {
    if docker_image_exists(image).await? {
        return Ok(());
    }

    if image != "nomos-testnet:local" {
        return Err(ComposeRunnerError::MissingImage {
            image: image.to_owned(),
        });
    }

    build_local_image(image, platform).await
}

/// Returns true when `docker image inspect` succeeds for the image.
pub async fn docker_image_exists(image: &str) -> Result<bool, ComposeRunnerError> {
    let mut cmd = Command::new("docker");
    cmd.arg("image")
        .arg("inspect")
        .arg(image)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match timeout(
        testing_framework_core::adjust_timeout(IMAGE_INSPECT_TIMEOUT),
        cmd.status(),
    )
    .await
    {
        Ok(Ok(status)) => Ok(status.success()),
        Ok(Err(source)) => Err(ComposeRunnerError::Compose(ComposeCommandError::Spawn {
            command: format!("docker image inspect {image}"),
            source,
        })),
        Err(_) => Err(ComposeRunnerError::Compose(ComposeCommandError::Timeout {
            command: format!("docker image inspect {image}"),
            timeout: testing_framework_core::adjust_timeout(IMAGE_INSPECT_TIMEOUT),
        })),
    }
}

/// Build the local testnet image with optional platform override.
pub async fn build_local_image(
    image: &str,
    platform: Option<&str>,
) -> Result<(), ComposeRunnerError> {
    let repo_root =
        repository_root().map_err(|source| ComposeRunnerError::ImageBuild { source })?;
    let dockerfile = repo_root.join("testing-framework/runners/docker/runner.Dockerfile");

    tracing::info!(image, "building compose runner docker image");

    let mut cmd = Command::new("docker");
    cmd.arg("build");

    if let Some(build_platform) = select_build_platform(platform)? {
        cmd.arg("--platform").arg(&build_platform);
    }

    let circuits_platform = env::var("COMPOSE_CIRCUITS_PLATFORM")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("linux-x86_64"));

    cmd.arg("--build-arg")
        .arg(format!("NOMOS_CIRCUITS_PLATFORM={circuits_platform}"));

    if let Some(value) = env::var("CIRCUITS_OVERRIDE")
        .ok()
        .filter(|val| !val.is_empty())
    {
        cmd.arg("--build-arg")
            .arg(format!("CIRCUITS_OVERRIDE={value}"));
    }

    cmd.arg("-t")
        .arg(image)
        .arg("-f")
        .arg(&dockerfile)
        .arg(&repo_root);

    run_docker_command(cmd, "docker build compose image", IMAGE_BUILD_TIMEOUT).await
}

/// Run a docker command with a timeout, mapping errors into runner errors.
pub async fn run_docker_command(
    mut command: Command,
    description: &str,
    timeout_duration: Duration,
) -> Result<(), ComposeRunnerError> {
    match timeout(timeout_duration, command.status()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(ComposeRunnerError::Compose(ComposeCommandError::Failed {
            command: description.to_owned(),
            status,
        })),
        Ok(Err(source)) => Err(ComposeRunnerError::Compose(ComposeCommandError::Spawn {
            command: description.to_owned(),
            source,
        })),
        Err(_) => Err(ComposeRunnerError::Compose(ComposeCommandError::Timeout {
            command: description.to_owned(),
            timeout: timeout_duration,
        })),
    }
}

fn detect_docker_platform() -> Result<Option<String>, ComposeRunnerError> {
    let output = StdCommand::new("docker")
        .arg("info")
        .arg("-f")
        .arg("{{.Architecture}}")
        .output()
        .map_err(|source| ComposeRunnerError::ImageBuild {
            source: source.into(),
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let arch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if arch.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!("linux/{arch}")))
}

/// Choose the build platform from user override or docker host architecture.
pub fn select_build_platform(
    requested: Option<&str>,
) -> Result<Option<String>, ComposeRunnerError> {
    if let Some(value) = requested {
        return Ok(Some(value.to_owned()));
    }

    detect_docker_platform()?.map_or_else(
        || {
            warn!("docker host architecture unavailable; letting docker choose default platform");
            Ok(None)
        },
        |host_platform| Ok(Some(host_platform)),
    )
}
