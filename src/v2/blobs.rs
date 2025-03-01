use crate::errors::{Error, Result};
use crate::v2::*;

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use pin_project::pin_project;
use reqwest::{self, Method, StatusCode};
use url::Url;

impl Client {
    #[inline]
    fn blob_url(
        &self,
        name: &str,
        digest: &str,
        ns: Option<&str>,
    ) -> core::result::Result<Url, url::ParseError> {
        let ep = match ns {
            Some(v) => format!("{}/v2/{}/blobs/{}?ns={}", self.base_url, name, digest, v),
            None => format!("{}/v2/{}/blobs/{}", self.base_url, name, digest),
        };
        Url::parse(&ep)
    }

    /// Check if a blob exists.
    pub async fn has_blob(&self, name: &str, digest: &str, ns: Option<&str>) -> Result<bool> {
        let url = self.blob_url(name, digest, ns)?;
        let res = self.build_reqwest(Method::HEAD, url).send().await?;

        trace!("Blob HEAD status: {:?}", res.status());

        match res.status() {
            StatusCode::OK => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn get_blob_response(
        &self,
        name: &str,
        digest: &str,
        ns: Option<&str>,
    ) -> Result<BlobResponse> {
        let url = self.blob_url(name, digest, ns)?;

        let resp = self.build_reqwest(Method::GET, url).send().await?;

        let status = resp.status();
        trace!("GET {} status: {}", resp.url(), status);

        match resp.error_for_status_ref() {
            Ok(_) => {
                if let Some(len) = resp.content_length() {
                    trace!("Receiving a blob with {} bytes", len);
                } else {
                    trace!("Receiving a blob");
                }
                Ok(BlobResponse::new(resp, ContentDigest::try_new(digest)?))
            }
            Err(_) if status.is_client_error() => Err(Error::Client { status }),
            Err(_) if status.is_server_error() => Err(Error::Server { status }),
            Err(_) => {
                error!("Received unexpected HTTP status '{}'", status);
                Err(Error::UnexpectedHttpStatus(status))
            }
        }
    }

    /// Retrieve blob.
    pub async fn get_blob(&self, name: &str, digest: &str, ns: Option<&str>) -> Result<Vec<u8>> {
        self.get_blob_response(name, digest, ns)
            .await?
            .bytes()
            .await
    }

    /// Retrieve blob stream.
    pub async fn get_blob_stream(
        &self,
        name: &str,
        digest: &str,
        ns: Option<&str>,
    ) -> Result<impl Stream<Item = Result<Bytes>>> {
        Ok(self.get_blob_response(name, digest, ns).await?.stream())
    }
}

#[derive(Debug)]
pub struct BlobResponse {
    resp: reqwest::Response,
    digest: ContentDigest,
}

impl BlobResponse {
    fn new(resp: reqwest::Response, digest: ContentDigest) -> Self {
        Self { resp, digest }
    }

    /// Get size of the blob.
    /// This method can be useful to render progress bar when downloading a blob.
    pub fn size(&self) -> Option<u64> {
        self.resp.content_length()
    }

    /// Retrieve content of the blob.
    pub async fn bytes(self) -> Result<Vec<u8>> {
        let blob = self.resp.bytes().await?.to_vec();

        let mut digest = self.digest;
        digest.update(&blob);
        digest.verify()?;

        Ok(blob)
    }

    /// Get bytes stream of the blob.
    pub fn stream(self) -> impl Stream<Item = Result<Bytes>> {
        BlobStream::new(self.resp.bytes_stream(), self.digest)
    }
}

#[pin_project]
struct BlobStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    #[pin]
    stream: S,
    #[pin]
    digest: Option<ContentDigest>,
}

impl<S> BlobStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Unpin,
{
    fn new(stream: S, digest: ContentDigest) -> Self {
        Self {
            stream,
            digest: Some(digest),
        }
    }
}

impl<S> Stream for BlobStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Unpin,
{
    type Item = Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match this.stream.poll_next(cx) {
            Poll::Ready(Some(chunk_res)) => {
                let mut digest = match this.digest.as_pin_mut() {
                    Some(digest) => digest,
                    None => return Poll::Ready(None),
                };
                let chunk = chunk_res?;
                digest.update(&chunk);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => match this.digest.take() {
                Some(digest) => match digest.verify() {
                    Ok(()) => Poll::Ready(None),
                    Err(err) => Poll::Ready(Some(Err(err.into()))),
                },
                None => Poll::Ready(None),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}
