use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{path::PathBuf, sync::atomic::AtomicBool};

pub mod cli;
pub mod io;
pub mod platform;
pub mod tool;

#[derive(Debug, Deserialize)]
pub struct UrlMirrorEntry {
    from: String,
    to: String,
}
#[derive(Debug, Default, Deserialize)]
pub struct UrlMirror {
    mirror: Vec<UrlMirrorEntry>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub mirror: Option<UrlMirror>,
    pub data_path: Option<PathBuf>,
}

pub async fn spawn_blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!("Failed to join spawned IO task")),
    }
}

pub struct HttpClient {
    mirror: UrlMirror,
    client_inner: reqwest::Client,
}

impl HttpClient {
    pub fn new(mirror: UrlMirror) -> HttpClient {
        HttpClient {
            mirror,
            client_inner: reqwest::Client::new(),
        }
    }

    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        for entry in &self.mirror.mirror {
            if let Some(rest) = url.strip_prefix(&entry.from) {
                let mut result = String::new();
                result.push_str(entry.to.as_str());
                result.push_str(rest);
                log::debug!("Applying mirror {} => {}", url, result);
                return self.client_inner.get(result);
            }
        }

        self.client_inner.get(url)
    }
}

pub enum Status {
    InProgress {
        name: SmolStr,
        progress_ratio: Option<(u64, u64)>,
    },
    Stopped,
}

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn set_cancelled() {
    CANCELLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_cancelled() -> bool {
    CANCELLED.load(std::sync::atomic::Ordering::Relaxed)
}

pub struct CancellableFuture<Fut> {
    inner: Fut,
}

impl<Fut> CancellableFuture<Fut> {
    pub fn new(inner: Fut) -> Self {
        CancellableFuture { inner }
    }
}

impl<Fut> Future for CancellableFuture<Fut>
where
    Fut: Future,
{
    type Output = Option<Fut::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if is_cancelled() {
            Poll::Ready(None)
        } else {
            // TODO: is unsafe right?
            let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
            match inner.poll(cx) {
                Poll::Ready(output) => Poll::Ready(Some(output)),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct FileHash {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<SmolStr>,
}
