use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    source_repo_root()
}

pub fn source_repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("src-tauri must live under app-scaffold")
        .to_path_buf()
}

pub fn resource_root() -> PathBuf {
    if let Some(path) = resource_root_override(std::env::var("BENCHFORGE_RESOURCE_DIR").ok()) {
        return path;
    }
    if let Some(path) = app_bundle_resource_root() {
        if path.join("benchmark-packs").exists() && path.join("adapters").exists() {
            return path;
        }
    }
    source_repo_root()
}

fn resource_root_override(value: Option<String>) -> Option<PathBuf> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn app_bundle_resource_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    app_bundle_resource_root_for_exe(&exe)
}

fn app_bundle_resource_root_for_exe(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name().and_then(|value| value.to_str()) == Some("MacOS") {
        let contents_dir = macos_dir.parent()?;
        if contents_dir.file_name().and_then(|value| value.to_str()) == Some("Contents") {
            let app_dir = contents_dir.parent()?;
            if app_dir.extension().and_then(|value| value.to_str()) != Some("app") {
                return None;
            }
            return Some(contents_dir.join("Resources"));
        }
    }
    None
}

pub fn app_data_dir() -> PathBuf {
    let override_path = app_data_dir_override(std::env::var("BENCHFORGE_DATA_DIR").ok());
    let current_exe = std::env::current_exe().ok();
    let source_root = source_repo_root();
    app_data_dir_for_context(override_path, current_exe.as_deref(), &source_root)
}

fn app_data_dir_for_context(
    override_path: Option<PathBuf>,
    current_exe: Option<&Path>,
    source_root: &Path,
) -> PathBuf {
    if let Some(path) = override_path {
        return path;
    }
    if current_exe
        .and_then(app_bundle_resource_root_for_exe)
        .is_some()
    {
        return platform_app_data_dir();
    }
    if source_root.join("benchmark-packs").exists() {
        return source_root.join(".benchforge");
    }
    platform_app_data_dir()
}

fn platform_app_data_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("BenchForge");
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("BenchForge");
        }
    }
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("benchforge");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("benchforge");
    }
    std::env::temp_dir().join("benchforge")
}

fn app_data_dir_override(value: Option<String>) -> Option<PathBuf> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn runs_dir() -> PathBuf {
    app_data_dir().join("runs")
}

pub fn exports_dir() -> PathBuf {
    app_data_dir().join("exports")
}

pub fn diagnostics_dir() -> PathBuf {
    app_data_dir().join("diagnostics")
}

pub fn db_path() -> PathBuf {
    app_data_dir().join("benchforge.sqlite")
}

pub fn worker_venv_launcher() -> PathBuf {
    resource_root()
        .join("workers")
        .join(".venv")
        .join("bin")
        .join("benchforge-worker")
}

pub fn bundled_worker_launcher() -> PathBuf {
    resource_root().join("workers").join("benchforge-worker")
}

pub fn bundled_worker_package_dir() -> PathBuf {
    resource_root().join("workers").join("benchforge_worker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn app_data_dir_override_ignores_blank_values() {
        assert!(app_data_dir_override(None).is_none());
        assert!(app_data_dir_override(Some("  ".into())).is_none());
    }

    #[test]
    fn app_data_dir_override_accepts_explicit_path() {
        assert_eq!(
            app_data_dir_override(Some("/tmp/benchforge-clean".into())),
            Some(PathBuf::from("/tmp/benchforge-clean"))
        );
    }

    #[test]
    fn resource_root_override_accepts_explicit_path() {
        assert_eq!(
            resource_root_override(Some("/tmp/benchforge-resources".into())),
            Some(PathBuf::from("/tmp/benchforge-resources"))
        );
    }

    #[test]
    fn resource_root_override_ignores_blank_values() {
        assert!(resource_root_override(None).is_none());
        assert!(resource_root_override(Some("  ".into())).is_none());
    }

    #[test]
    fn app_bundle_resource_root_detects_macos_app_executable() {
        assert_eq!(
            app_bundle_resource_root_for_exe(Path::new(
                "/Applications/BenchForge.app/Contents/MacOS/benchforge"
            )),
            Some(PathBuf::from(
                "/Applications/BenchForge.app/Contents/Resources"
            ))
        );
    }

    #[test]
    fn app_bundle_resource_root_rejects_non_app_contents_path() {
        assert!(
            app_bundle_resource_root_for_exe(Path::new("/tmp/Contents/MacOS/benchforge")).is_none()
        );
    }

    #[test]
    fn app_bundle_data_dir_uses_platform_location_even_when_source_exists() {
        let source_root =
            std::env::temp_dir().join(format!("benchforge-source-root-{}", std::process::id()));
        fs::create_dir_all(source_root.join("benchmark-packs")).expect("source root should exist");

        let data_dir = app_data_dir_for_context(
            None,
            Some(Path::new(
                "/Applications/BenchForge.app/Contents/MacOS/benchforge",
            )),
            &source_root,
        );

        assert_ne!(data_dir, source_root.join(".benchforge"));
        if cfg!(target_os = "macos") {
            assert!(data_dir.ends_with(
                Path::new("Library")
                    .join("Application Support")
                    .join("BenchForge")
            ));
        }

        let _ = fs::remove_dir_all(source_root);
    }

    #[test]
    fn worker_resource_paths_live_under_resource_root() {
        let root = resource_root().join("workers");

        assert_eq!(
            worker_venv_launcher(),
            root.join(".venv/bin/benchforge-worker")
        );
        assert_eq!(bundled_worker_launcher(), root.join("benchforge-worker"));
        assert_eq!(bundled_worker_package_dir(), root.join("benchforge_worker"));
    }
}
