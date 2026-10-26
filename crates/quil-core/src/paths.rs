use std::path::PathBuf;

/// Per-user model storage without reading any configuration or environment
/// files. `HOME` is used only to locate the macOS application-support folder.
pub fn default_model_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Quil")
                .join("models"),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
