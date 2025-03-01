//! Media-types for API objects.

use crate::errors::Result;
use serde_with::DeserializeFromStr;
use serde_with::SerializeDisplay;
use strum::EnumProperty;

// For schema1 types, see https://docs.docker.com/registry/spec/manifest-v2-1/
// For schema2 types, see https://docs.docker.com/registry/spec/manifest-v2-2/

#[derive(
    EnumProperty,
    EnumString,
    Display,
    Debug,
    Hash,
    PartialEq,
    Eq,
    Clone,
    DeserializeFromStr,
    SerializeDisplay,
)]
pub enum MediaTypes {
    /// OCI image, version 1
    #[strum(serialize = "application/vnd.oci.image.index.v1+json")]
    #[strum(props(Sub = "vnd.oci.image.index.v1+json"))]
    OciV1ManifestList,
    /// OCI manifest, version 1
    #[strum(serialize = "application/vnd.oci.image.manifest.v1+json")]
    #[strum(props(Sub = "vnd.oci.image.manifest.v1+json"))]
    OciV1Manifest,
    /// OCI manifest config, version 1
    #[strum(serialize = "application/vnd.oci.image.config.v1+json")]
    #[strum(props(Sub = "vnd.oci.image.config.v1+json"))]
    OciV1ManifestConfig,
    /// Manifest, version 2 schema 1.
    #[strum(serialize = "application/vnd.docker.distribution.manifest.v1+json")]
    #[strum(props(Sub = "vnd.docker.distribution.manifest.v1+json"))]
    ManifestV2S1,
    /// Signed manifest, version 2 schema 1.
    #[strum(serialize = "application/vnd.docker.distribution.manifest.v1+prettyjws")]
    #[strum(props(Sub = "vnd.docker.distribution.manifest.v1+prettyjws"))]
    ManifestV2S1Signed,
    /// Manifest, version 2 schema 2.
    #[strum(serialize = "application/vnd.docker.distribution.manifest.v2+json")]
    #[strum(props(Sub = "vnd.docker.distribution.manifest.v2+json"))]
    ManifestV2S2,
    /// Manifest List (aka "fat manifest").
    #[strum(serialize = "application/vnd.docker.distribution.manifest.list.v2+json")]
    #[strum(props(Sub = "vnd.docker.distribution.manifest.list.v2+json"))]
    ManifestList,
    /// Image layer, as a gzip-compressed tar.
    #[strum(serialize = "application/vnd.docker.image.rootfs.diff.tar.gzip")]
    #[strum(props(Sub = "vnd.docker.image.rootfs.diff.tar.gzip"))]
    ImageLayerTgz,
    /// Configuration object for a container.
    #[strum(serialize = "application/vnd.docker.container.image.v1+json")]
    #[strum(props(Sub = "vnd.docker.container.image.v1+json"))]
    ContainerConfigV1,
    /// Generic JSON
    #[strum(serialize = "application/json")]
    #[strum(props(Sub = "json"))]
    ApplicationJson,
}

impl MediaTypes {
    // TODO(lucab): proper error types
    pub fn from_mime(mtype: &mime::Mime) -> Result<Self> {
        match (mtype.type_(), mtype.subtype(), mtype.suffix()) {
            (mime::APPLICATION, mime::JSON, _) => Ok(MediaTypes::ApplicationJson),
            (mime::APPLICATION, subt, Some(suff)) => match (subt.as_str(), suff.as_str()) {
                ("vnd.docker.distribution.manifest.v1", "json") => Ok(MediaTypes::ManifestV2S1),
                ("vnd.docker.distribution.manifest.v1", "prettyjws") => {
                    Ok(MediaTypes::ManifestV2S1Signed)
                }
                ("vnd.docker.distribution.manifest.v2", "json") => Ok(MediaTypes::ManifestV2S2),
                ("vnd.docker.distribution.manifest.list.v2", "json") => {
                    Ok(MediaTypes::ManifestList)
                }
                ("vnd.docker.image.rootfs.diff.tar.gzip", _) => Ok(MediaTypes::ImageLayerTgz),
                ("vnd.docker.container.image.v1", "json") => Ok(MediaTypes::ContainerConfigV1),
                _ => Err(crate::Error::UnknownMimeType(mtype.clone())),
            },
            _ => Err(crate::Error::UnknownMimeType(mtype.clone())),
        }
    }
    pub fn to_mime(&self) -> mime::Mime {
        match self {
            &MediaTypes::ApplicationJson => Ok(mime::APPLICATION_JSON),
            m => match m.get_str("Sub") {
                Some(s) => format!("application/{s}").parse(),
                None => "application/star".parse(),
            },
        }
        .expect("to_mime should be always successful")
    }
}
