use crate::errors::{Error, Result};
use crate::mediatypes::MediaTypes;
use compact_str::CompactString;
use reqwest::Method;

/// Manifest version 2 schema 2.
///
/// Specification is at <https://docs.docker.com/registry/spec/manifest-v2-2/>.
#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestSchema2Spec {
    #[serde(rename = "schemaVersion")]
    schema_version: u16,
    #[serde(rename = "mediaType")]
    media_type: MediaTypes,
    config: Config,
    layers: Vec<S2Layer>,
}

/// Super-type for combining a ManifestSchema2 with a ConfigBlob.
#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestSchema2 {
    #[serde(flatten)]
    pub manifest_spec: ManifestSchema2Spec,
    #[serde(skip)]
    pub config_blob: ConfigBlob,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(rename = "mediaType")]
    pub media_type: CompactString,
    pub size: u64,
    pub digest: String,
}

/// Partial representation of a container image (application/vnd.docker.container.image.v1+json).
///
/// The remaining fields according to [the image spec v1][image-spec-v1] are not covered.
///
/// [image-spec-v1]: https://github.com/moby/moby/blob/a30990b3c8d0d42280fa501287859e1d2393a951/image/spec/v1.md#image-json-description
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ConfigBlob {
    architecture: CompactString,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct S2Layer {
    #[serde(rename = "mediaType")]
    media_type: CompactString,
    size: u64,
    digest: String,
    urls: Option<Vec<String>>,
}

/// Manifest List.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ManifestList {
    #[serde(rename = "schemaVersion")]
    schema_version: u16,
    #[serde(rename = "mediaType")]
    media_type: CompactString,
    pub manifests: Vec<ManifestObj>,
}

/// Manifest object.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ManifestObj {
    #[serde(rename = "mediaType")]
    media_type: CompactString,
    size: u64,
    pub digest: String,
    pub platform: Platform,
}

/// Platform-related manifest entries.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Platform {
    pub architecture: CompactString,
    pub os: CompactString,
    #[serde(rename = "os.version")]
    pub os_version: Option<CompactString>,
    #[serde(rename = "os.features")]
    pub os_features: Option<Vec<CompactString>>,
    pub variant: Option<CompactString>,
    pub features: Option<Vec<CompactString>>,
}

impl ManifestSchema2Spec {
    /// Get `Config` object referenced by this manifest.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Fetch the config blob for this manifest
    pub(crate) async fn fetch_config_blob(
        self,
        client: &crate::v2::Client,
        repo: &str,
    ) -> Result<ManifestSchema2> {
        let ep = format!(
            "{}/v2/{}/blobs/{}",
            &client.base_url, repo, self.config.digest
        );
        let url = reqwest::Url::parse(&ep)?;

        let r = client.build_reqwest(Method::GET, url).send().await?;

        let status = r.status();
        trace!("GET {:?}: {}", ep, &status);

        if !status.is_success() {
            return Err(Error::UnexpectedHttpStatus(status));
        }

        let config_blob = r.json::<ConfigBlob>().await?;

        Ok(ManifestSchema2 {
            manifest_spec: self,
            config_blob,
        })
    }
}

impl ManifestSchema2 {
    /// List digests of all layers referenced by this manifest.
    ///
    /// The returned layers list is ordered starting with the base image first.
    pub fn get_layers(&self) -> impl Iterator<Item = &str> {
        self.manifest_spec.layers.iter().map(|l| l.digest.as_ref())
    }

    /// Get the architecture from the config
    pub fn architecture(&self) -> &str {
        self.config_blob.architecture.as_ref()
    }
}

impl ManifestObj {
    /// Get the architecture of the manifest object
    pub fn architecture(&self) -> &str {
        self.platform.architecture.as_ref()
    }

    /// Returns the sha digest of the manifest object
    pub fn digest(&self) -> &str {
        self.digest.as_ref()
    }
}

impl ManifestList {
    /// Get architecture of all the manifests
    pub fn architectures(&self) -> impl Iterator<Item = &str> {
        self.manifests.iter().map(|mo| mo.architecture())
    }

    /// Get the digest for all the manifest images in the ManifestList
    pub fn get_digests(&self) -> impl Iterator<Item = &str> {
        self.manifests.iter().map(|mo| mo.digest())
    }
}
