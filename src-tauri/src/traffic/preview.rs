use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;

pub struct PreviewBuf {
    data: Mutex<Vec<u8>>,
    limit: usize,
    truncated: AtomicBool,
}

impl PreviewBuf {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(Vec::new()),
            limit,
            truncated: AtomicBool::new(false),
        })
    }

    pub fn append(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let mut data = self.data.lock().expect("preview lock");
        if data.len() >= self.limit {
            self.truncated.store(true, Ordering::Relaxed);
            return;
        }
        let room = self.limit - data.len();
        let take = room.min(chunk.len());
        data.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            self.truncated.store(true, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> (String, bool) {
        let data = self.data.lock().expect("preview lock");
        (String::from_utf8_lossy(&data).into_owned(), self.truncated.load(Ordering::Relaxed))
    }
}

pin_project_lite::pin_project! {
    pub struct PreviewBody<B> {
        #[pin]
        inner: B,
        preview: Arc<PreviewBuf>,
        on_end: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }
}

impl<B> PreviewBody<B> {
    pub fn new(inner: B, preview: Arc<PreviewBuf>, on_end: impl FnOnce() + Send + 'static) -> Self {
        Self {
            inner,
            preview,
            on_end: Mutex::new(Some(Box::new(on_end))),
        }
    }
}

impl<B> http_body::Body for PreviewBody<B>
where
    B: http_body::Body<Data = Bytes>,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        let mut this = self.project();
        match this.inner.as_mut().poll_frame(cx) {
            std::task::Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.preview.append(data);
                }
                std::task::Poll::Ready(Some(Ok(frame)))
            }
            std::task::Poll::Ready(None) => {
                if let Some(callback) = this.on_end.lock().expect("preview callback").take() {
                    callback();
                }
                std::task::Poll::Ready(None)
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
