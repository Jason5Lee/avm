use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{path::PathBuf, sync::atomic::AtomicBool};

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
    mirrors: Vec<UrlMirrorEntry>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DefaultPlatform {
    pub global: Option<String>,
    #[serde(flatten)]
    pub tools: FxHashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub mirrors: Option<UrlMirror>,
    pub data_path: Option<PathBuf>,
    #[serde(rename = "default-platform")]
    pub default_platform: Option<DefaultPlatform>,
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
        for entry in &self.mirror.mirrors {
            if let Some(rest) = url.strip_prefix(&entry.from) {
                let mut result = String::new();
                result.push_str(entry.to.as_str());
                result.push_str(rest);
                log::debug!("Applied mirror {} => {}", url, result);
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tag(SmolStr);
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagStr<'a>(&'a str);

impl<'a> Deref for TagStr<'a> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl Deref for Tag {
    type Target = SmolStr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Tag {
    pub fn as_tag_str(&self) -> TagStr<'_> {
        TagStr(self.0.as_str())
    }
}

pub struct TagIsNotValid(char);

impl fmt::Display for TagIsNotValid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tag contains invalid character: {:?}", self.0)
    }
}

impl fmt::Debug for TagIsNotValid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for TagIsNotValid {}

impl<'a> TryFrom<&'a str> for TagStr<'a> {
    type Error = TagIsNotValid;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        if let Some(c) = value.chars().find(|&c| {
            c == '/'
                || c == '\\'
                || c == '\0'
                || c.is_control()
                || c == '<'
                || c == '>'
                || c == ':'
                || c == '"'
                || c == '|'
                || c == '?'
                || c == '*'
        }) {
            return Err(TagIsNotValid(c));
        }

        Ok(TagStr(value))
    }
}

impl TryFrom<SmolStr> for Tag {
    type Error = TagIsNotValid;

    fn try_from(value: SmolStr) -> Result<Self, Self::Error> {
        match TagStr::try_from(value.as_str()) {
            Ok(_) => Ok(Tag(value)),
            Err(e) => Err(e),
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct FileHash {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha512: Option<SmolStr>,
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
            // SAFETY: CancellableFuture does not move inner after being pinned, and this
            // projection only creates a pinned mutable reference to that field.
            let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
            match inner.poll(cx) {
                Poll::Ready(output) => Poll::Ready(Some(output)),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}
