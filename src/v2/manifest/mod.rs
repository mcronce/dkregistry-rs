use crate::errors::{Error, Result};
use crate::mediatypes::MediaTypes;
use crate::v2::*;
use bytes::Bytes;
use itertools::Either;
use reqwest::{self, header, StatusCode};
use std::iter::FromIterator;
use std::str::FromStr;
use url::Url;

mod manifest_schema1;
pub use self::manifest_schema1::*;

mod manifest_schema2;
pub use self::manifest_schema2::{
    ConfigBlob, ManifestList, ManifestObj, ManifestSchema2, ManifestSchema2Spec, Platform,
};

impl Client {
    #[inline]
    fn manifest_url(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
    ) -> core::result::Result<Url, url::ParseError> {
        let ep = match ns {
            Some(v) => format!(
                "{}/v2/{}/manifests/{}?ns={}",
                self.base_url, name, reference, v
            ),
            None => format!("{}/v2/{}/manifests/{}", self.base_url, name, reference),
        };
        reqwest::Url::parse(&ep)
    }

    /// Fetch an image manifest.
    ///
    /// The name and reference parameters identify the image.
    /// The reference may be either a tag or digest.
    pub async fn get_manifest(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
    ) -> Result<Manifest> {
        self.get_manifest_and_ref(name, reference, ns)
            .await
            .map(|(manifest, _)| manifest)
    }

    /// Fetch an image manifest and return it with its digest.
    ///
    /// The name and reference parameters identify the image.
    /// The reference may be either a tag or digest.
    pub async fn get_manifest_and_ref(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
    ) -> Result<(Manifest, Option<String>)> {
        let (body, media_type, content_digest) = self
            .get_raw_manifest_and_metadata(name, reference, ns)
            .await?;

        match media_type {
            MediaTypes::ManifestV2S1Signed => Ok((
                Manifest::S1Signed(serde_json::from_slice(body.as_ref())?),
                content_digest,
            )),
            MediaTypes::ManifestV2S2 => {
                let m: ManifestSchema2Spec = serde_json::from_slice(body.as_ref())?;
                Ok((
                    m.fetch_config_blob(self, name).await.map(Manifest::S2)?,
                    content_digest,
                ))
            }
            MediaTypes::ManifestList => Ok((
                Manifest::ML(serde_json::from_slice(body.as_ref())?),
                content_digest,
            )),
            unsupported => Err(Error::UnsupportedMediaType(unsupported)),
        }
    }

    pub async fn get_raw_manifest_and_metadata(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
    ) -> Result<(Bytes, MediaTypes, Option<String>)> {
        let url = self.manifest_url(name, reference, ns)?;

        let accept_headers = build_accept_headers(&self.accepted_types);

        let res = self
            .build_reqwest(Method::GET, url)
            .headers(accept_headers)
            .send()
            .await?;

        let status = res.status();
        trace!("GET '{}' status: {:?}", res.url(), status);

        match status {
            StatusCode::OK => {}
            _ => return Err(Error::UnexpectedHttpStatus(status)),
        }

        let headers = res.headers();
        let content_digest =
            match headers.get(header::HeaderName::from_static("docker-content-digest")) {
                Some(content_digest_value) => Some(content_digest_value.to_str()?.to_string()),
                None => {
                    debug!("cannot find manifestref in headers");
                    None
                }
            };

        let header_content_type = headers.get(header::CONTENT_TYPE);
        let media_type = evaluate_media_type(header_content_type, res.url())?;

        trace!(
            "content-type: {:?}, media-type: {:?}",
            header_content_type,
            media_type
        );

        Ok((res.bytes().await?, media_type, content_digest))
    }

    /// Fetch content digest for a particular tag.
    pub async fn get_manifestref(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
    ) -> Result<Option<String>> {
        let url = self.manifest_url(name, reference, ns)?;

        let accept_headers = build_accept_headers(&self.accepted_types);

        let res = self
            .build_reqwest(Method::HEAD, url)
            .headers(accept_headers)
            .send()
            .await?;

        let status = res.status();
        trace!("HEAD '{}' status: {:?}", res.url(), status);

        match status {
            StatusCode::OK => {}
            _ => return Err(Error::UnexpectedHttpStatus(status)),
        }

        let headers = res.headers();
        let content_digest = match headers.get("docker-content-digest") {
            Some(content_digest_value) => Some(content_digest_value.to_str()?.to_string()),
            None => {
                debug!("cannot find manifestref in headers");
                None
            }
        };
        Ok(content_digest)
    }

    /// Check if an image manifest exists.
    ///
    /// The name and reference parameters identify the image.
    /// The reference may be either a tag or digest.
    pub async fn has_manifest(
        &self,
        name: &str,
        reference: &str,
        ns: Option<&str>,
        mediatypes: Option<&[&str]>,
    ) -> Result<Option<MediaTypes>> {
        let url = self.manifest_url(name, reference, ns)?;
        let accept_types = match mediatypes {
            None => {
                let m = MediaTypes::ManifestV2S2.to_mime();
                Either::Left(std::iter::once(m))
            }
            Some(v) => Either::Right(to_mimes(v)),
        };

        let mut accept_headers = header::HeaderMap::with_capacity(accept_types.size_hint().0);
        for accept_type in accept_types {
            let header_value = header::HeaderValue::from_str(accept_type.as_ref())
                .expect("mime type is always valid header value");
            accept_headers.insert(header::ACCEPT, header_value);
        }

        trace!("HEAD {:?}", url);

        let r = self
            .build_reqwest(Method::HEAD, url)
            .headers(accept_headers)
            .send()
            .await
            .map_err(Error::from)?;

        let status = r.status();

        trace!(
            "Manifest check status '{:?}', headers '{:?}",
            r.status(),
            r.headers(),
        );

        match status {
            StatusCode::MOVED_PERMANENTLY
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::FOUND
            | StatusCode::OK => {
                let media_type =
                    evaluate_media_type(r.headers().get(header::CONTENT_TYPE), r.url())?;
                trace!("Manifest media-type: {:?}", media_type);
                Ok(Some(media_type))
            }
            StatusCode::NOT_FOUND => Ok(None),
            _ => Err(Error::UnexpectedHttpStatus(status)),
        }
    }
}

fn to_mimes<'a>(v: &'a [&'a str]) -> impl Iterator<Item = mime::Mime> + 'a {
    let res = v.iter().filter_map(|x| {
        let mtype = MediaTypes::from_str(x);
        match mtype {
            Ok(m) => Some(m.to_mime()),
            _ => None,
        }
    });
    res
}

// Evaluate the `MediaTypes` from the the request header.
fn evaluate_media_type(
    content_type: Option<&reqwest::header::HeaderValue>,
    url: &Url,
) -> Result<MediaTypes> {
    let header_content_type = content_type
        .map(|hv| hv.to_str())
        .map(std::result::Result::unwrap_or_default);

    let is_pulp_based = url.path().starts_with("/pulp/docker/v2");

    match (header_content_type, is_pulp_based) {
        (Some(header_value), false) => MediaTypes::from_str(header_value).map_err(Into::into),
        (None, false) => Err(Error::MediaTypeSniff),
        (Some(header_value), true) => {
            // TODO: remove this workaround once Satellite returns a proper content-type here
            match header_value {
                "application/x-troff-man" => {
                    trace!("Applying workaround for pulp-based registries, e.g. Satellite");
                    MediaTypes::from_str(
                        "application/vnd.docker.distribution.manifest.v1+prettyjws",
                    )
                    .map_err(Into::into)
                }
                _ => {
                    debug!("Received content-type '{}' from pulp-based registry. Feeling lucky and trying to parse it...", header_value);
                    MediaTypes::from_str(header_value).map_err(Into::into)
                }
            }
        }
        (None, true) => {
            trace!("Applying workaround for pulp-based registries, e.g. Satellite");
            MediaTypes::from_str("application/vnd.docker.distribution.manifest.v1+prettyjws")
                .map_err(Into::into)
        }
    }
}

fn build_accept_headers(accepted_types: &[(MediaTypes, Option<f64>)]) -> header::HeaderMap {
    let accepted_types_string = accepted_types
        .iter()
        .map(|(ty, q)| {
            format!(
                "{}{}",
                ty,
                match q {
                    None => String::default(),
                    Some(v) => format!("; q={}", v),
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    header::HeaderMap::from_iter(vec![(
        header::ACCEPT,
        header::HeaderValue::from_str(&accepted_types_string).expect(
            "should be always valid because both float and mime type only use allowed ASCII chard",
        ),
    )])
}

/// Umbrella type for common actions on the different manifest schema types
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Manifest {
    S1Signed(manifest_schema1::ManifestSchema1Signed),
    S2(manifest_schema2::ManifestSchema2),
    ML(manifest_schema2::ManifestList),
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("no architecture in manifest")]
    NoArchitecture,
    #[error("architecture mismatch")]
    ArchitectureMismatch,
    #[error("manifest {0} does not support the 'layer_digests' method")]
    LayerDigestsUnsupported(String),
    #[error("manifest {0} does not support the 'architecture' method")]
    ArchitectureNotSupported(String),
}

impl Manifest {
    /// List digests of all layers referenced by this manifest, if available.
    /// For ManifestList, returns the digests of all the manifest list images.
    ///
    /// As manifest list images only contain digests of the
    /// images contained in the manifest, the `layers_digests`
    /// function returns the digests of all the images
    /// contained in the ManifestList instead of individual
    /// layers of the manifests.
    /// The layers of a specific image from manifest list can
    /// be obtained using the digest of the image from the
    /// manifest list and getting its manifest and manifestref
    /// (get_manifest_and_ref()) and using this manifest of
    /// the individual image to get the layers.
    ///
    /// The returned layers list for non ManifestList images is ordered starting with the base image first.
    pub fn layers_digests(&self, architecture: Option<&str>) -> Result<impl Iterator<Item = &str>> {
        match (self, self.architectures(), architecture) {
            (Manifest::S1Signed(m), _, None) => Ok(Either::Left(Either::Left(m.get_layers()))),
            (Manifest::S2(m), _, None) => Ok(Either::Left(Either::Right(m.get_layers()))),
            (Manifest::S1Signed(m), Ok(mut self_architectures), Some(a)) => {
                let self_a = self_architectures
                    .next()
                    .ok_or(ManifestError::NoArchitecture)?;
                if self_a != a {
                    return Err(ManifestError::ArchitectureMismatch.into());
                }
                Ok(Either::Right(Either::Left(m.get_layers())))
            }
            (Manifest::S2(m), Ok(mut self_architectures), Some(a)) => {
                let self_a = self_architectures
                    .next()
                    .ok_or(ManifestError::NoArchitecture)?;
                if self_a != a {
                    return Err(ManifestError::ArchitectureMismatch.into());
                }
                Ok(Either::Right(Either::Right(Either::Left(m.get_layers()))))
            }
            (Manifest::ML(m), _, _) => {
                Ok(Either::Right(Either::Right(Either::Right(m.get_digests()))))
            }
            _ => Err(ManifestError::LayerDigestsUnsupported(format!("{:?}", self)).into()),
        }
    }

    /// The architectures of the image the manifest points to, if available.
    pub fn architectures(&self) -> Result<impl Iterator<Item = &str>> {
        match self {
            Manifest::S1Signed(m) => Ok(Either::Left(std::iter::once(m.architecture.as_ref()))),
            Manifest::S2(m) => Ok(Either::Left(std::iter::once(m.architecture()))),
            Manifest::ML(m) => Ok(Either::Right(m.architectures())),
        }
    }

    /// Media type of manifest
    pub fn media_type(&self) -> MediaTypes {
        match self {
            Manifest::S1Signed(_) => MediaTypes::ManifestV2S1Signed,
            Manifest::S2(_) => MediaTypes::ManifestV2S2,
            Manifest::ML(_) => MediaTypes::ManifestList,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    use crate::v2::Client;

    #[test_case("not-gcr.io" => "application/vnd.docker.distribution.manifest.v2+json; q=0.5,application/vnd.docker.distribution.manifest.v1+prettyjws; q=0.4,application/vnd.docker.distribution.manifest.list.v2+json; q=0.5"; "Not gcr registry")]
    #[test_case("gcr.io" => "application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.v1+prettyjws,application/vnd.docker.distribution.manifest.list.v2+json"; "gcr.io")]
    #[test_case("foobar.gcr.io" => "application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.v1+prettyjws,application/vnd.docker.distribution.manifest.list.v2+json"; "Custom gcr.io registry")]
    fn gcr_io_accept_headers(registry: &str) -> String {
        let client_builder = Client::configure().registry(&registry);
        let client = client_builder.build().unwrap();
        let header_map = build_accept_headers(&client.accepted_types);
        header_map
            .get(header::ACCEPT)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }
    #[test_case(None => "application/vnd.docker.distribution.manifest.v2+json; q=0.5,application/vnd.docker.distribution.manifest.v1+prettyjws; q=0.4,application/vnd.docker.distribution.manifest.list.v2+json; q=0.5"; "Default settings")]
    #[test_case(Some(vec![
        (MediaTypes::ManifestV2S2, Some(0.5)),
        (MediaTypes::ManifestV2S1Signed, Some(0.2)),
        (MediaTypes::ManifestList, Some(0.5)),
    ]) => "application/vnd.docker.distribution.manifest.v2+json; q=0.5,application/vnd.docker.distribution.manifest.v1+prettyjws; q=0.2,application/vnd.docker.distribution.manifest.list.v2+json; q=0.5"; "Custom accept types with weight")]
    #[test_case(Some(vec![
        (MediaTypes::ManifestV2S2, None),
        (MediaTypes::ManifestList, None),
    ]) => "application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json"; "Custom accept types, no weight")]
    fn custom_accept_headers(accept_headers: Option<Vec<(MediaTypes, Option<f64>)>>) -> String {
        let registry = "https://example.com";

        let client_builder = Client::configure()
            .registry(&registry)
            .accepted_types(accept_headers);
        let client = client_builder.build().unwrap();
        let header_map = build_accept_headers(&client.accepted_types);
        header_map
            .get(header::ACCEPT)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }
}
