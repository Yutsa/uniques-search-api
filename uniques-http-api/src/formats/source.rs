use std::path::{Path, PathBuf};

use crate::config::FormatsSourceConfig;

#[derive(Debug, Clone)]
pub struct DiskFormatsSource {
    root: PathBuf,
}

impl DiskFormatsSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug, Clone)]
pub struct HttpFormatsSource {
    manifest_url: String,
}

impl HttpFormatsSource {
    pub fn new(manifest_url: impl Into<String>) -> Self {
        Self {
            manifest_url: manifest_url.into(),
        }
    }

    pub fn manifest_url(&self) -> &str {
        &self.manifest_url
    }

    /// URL for a manifest entry's `path`, resolved against the manifest's own URL
    /// (i.e. sibling of `manifest.json`).
    pub fn file_url(&self, relative_path: &str) -> String {
        let base_end = self.manifest_url.rfind('/').map_or(0, |idx| idx + 1);
        format!("{}{relative_path}", &self.manifest_url[..base_end])
    }
}

#[derive(Debug, Clone)]
pub enum FormatsSource {
    Disk(DiskFormatsSource),
    Http(HttpFormatsSource),
}

impl FormatsSource {
    pub fn from_config(cfg: &FormatsSourceConfig) -> Self {
        match cfg {
            FormatsSourceConfig::Disk { path } => Self::Disk(DiskFormatsSource::new(path.clone())),
            FormatsSourceConfig::Http { manifest_url } => {
                Self::Http(HttpFormatsSource::new(manifest_url.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_resolves_sibling_of_manifest() {
        let source = HttpFormatsSource::new(
            "https://altered-reunion-formats-prod.s3.fr-par.scw.cloud/manifest.json",
        );
        assert_eq!(
            source.file_url("standard.json"),
            "https://altered-reunion-formats-prod.s3.fr-par.scw.cloud/standard.json"
        );
    }

    #[test]
    fn file_url_resolves_nested_manifest_prefix() {
        let source = HttpFormatsSource::new("https://example.com/formats/v1/manifest.json");
        assert_eq!(
            source.file_url("draft.json"),
            "https://example.com/formats/v1/draft.json"
        );
    }
}
