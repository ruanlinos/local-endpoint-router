use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hyper::HeaderMap;
use serde::Serialize;

use crate::traffic::preview::PreviewBuf;

const RING_LIMIT: usize = 500;
pub const PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficEvent {
    pub id: u64,
    pub timestamp: String,
    pub method: String,
    pub url: String,
    pub host: String,
    pub status: Option<u16>,
    pub mapped: bool,
    pub rule_id: Option<String>,
    pub rule_name: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub kind: String,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub request_body_preview: String,
    pub response_body_preview: String,
    pub request_truncated: bool,
    pub response_truncated: bool,
}

struct Entry {
    id: u64,
    started: Instant,
    timestamp: String,
    method: String,
    url: String,
    host: String,
    status: Option<u16>,
    mapped: bool,
    rule_id: Option<String>,
    rule_name: Option<String>,
    error: Option<String>,
    kind: String,
    request_headers: Vec<(String, String)>,
    response_headers: Vec<(String, String)>,
    request_preview: Arc<PreviewBuf>,
    response_preview: Arc<PreviewBuf>,
    finished: Option<u64>,
}

pub struct TrafficCollector {
    entries: Mutex<VecDeque<Entry>>,
    next_id: AtomicU64,
    redact: AtomicBool,
    capture_limit: AtomicU64,
    app: Mutex<Option<tauri::AppHandle>>,
}

impl TrafficCollector {
    pub fn new(redact: bool, capture_limit: usize) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            redact: AtomicBool::new(redact),
            capture_limit: AtomicU64::new(capture_limit as u64),
            app: Mutex::new(None),
        })
    }

    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().expect("traffic app lock") = Some(app);
    }

    pub fn configure(&self, redact: bool, capture_limit: usize) {
        self.redact.store(redact, Ordering::Relaxed);
        self.capture_limit.store(capture_limit as u64, Ordering::Relaxed);
    }

    pub fn capture_limit(&self) -> usize {
        self.capture_limit.load(Ordering::Relaxed) as usize
    }

    pub fn open(&self, method: &str, url: &str, host: &str, kind: &str) -> OpenExchange {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let limit = self.capture_limit();
        let request_preview = PreviewBuf::new(limit);
        let response_preview = PreviewBuf::new(limit);
        let entry = Entry {
            id,
            started: Instant::now(),
            timestamp: crate::util::now_iso(),
            method: method.to_string(),
            url: url.to_string(),
            host: host.to_string(),
            status: None,
            mapped: false,
            rule_id: None,
            rule_name: None,
            error: None,
            kind: kind.to_string(),
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            request_preview: request_preview.clone(),
            response_preview: response_preview.clone(),
            finished: None,
        };
        self.push(entry);
        OpenExchange {
            id,
            request_preview,
            response_preview,
        }
    }

    pub fn set_headers(
        &self,
        id: u64,
        request_headers: &HeaderMap,
        mapped: bool,
        rule_id: Option<String>,
        rule_name: Option<String>,
    ) {
        let redact = self.redact.load(Ordering::Relaxed);
        self.with_entry(id, |entry| {
            entry.request_headers = redact_headers(request_headers, redact);
            entry.mapped = mapped;
            entry.rule_id = rule_id;
            entry.rule_name = rule_name;
        });
    }

    pub fn finish(&self, id: u64, status: Option<u16>, response_headers: &HeaderMap, error: Option<String>) {
        let redact = self.redact.load(Ordering::Relaxed);
        self.with_entry(id, |entry| {
            entry.status = status;
            entry.response_headers = redact_headers(response_headers, redact);
            entry.error = error;
            entry.finished = Some(entry.started.elapsed().as_millis() as u64);
        });
        self.emit(id);
    }

    pub fn fail(&self, method: &str, url: &str, host: &str, status: u16, error: &str, kind: &str) {
        let opened = self.open(method, url, host, kind);
        self.finish(opened.id, Some(status), &HeaderMap::new(), Some(error.to_string()));
    }

    pub fn emit(&self, id: u64) {
        let Some(event) = self.snapshot(id) else {
            return;
        };
        let Ok(app) = self.app.lock() else {
            return;
        };
        if let Some(app) = app.as_ref() {
            use tauri::Emitter;
            let _ = app.emit("traffic://event", &event);
        }
    }

    pub fn list(&self) -> Vec<TrafficEvent> {
        self.entries
            .lock()
            .expect("traffic lock")
            .iter()
            .rev()
            .map(Entry::snapshot)
            .collect()
    }

    pub fn clear(&self) {
        self.entries.lock().expect("traffic lock").clear();
    }

    fn push(&self, entry: Entry) {
        let mut entries = self.entries.lock().expect("traffic lock");
        entries.push_back(entry);
        while entries.len() > RING_LIMIT {
            entries.pop_front();
        }
    }

    fn with_entry(&self, id: u64, update: impl FnOnce(&mut Entry)) {
        let mut entries = self.entries.lock().expect("traffic lock");
        if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
            update(entry);
        }
    }

    fn snapshot(&self, id: u64) -> Option<TrafficEvent> {
        self.entries
            .lock()
            .expect("traffic lock")
            .iter()
            .find(|entry| entry.id == id)
            .map(Entry::snapshot)
    }
}

pub struct OpenExchange {
    pub id: u64,
    pub request_preview: Arc<PreviewBuf>,
    pub response_preview: Arc<PreviewBuf>,
}

impl Entry {
    fn snapshot(&self) -> TrafficEvent {
        let (request_body_preview, request_truncated) = self.request_preview.snapshot();
        let (response_body_preview, response_truncated) = self.response_preview.snapshot();
        TrafficEvent {
            id: self.id,
            timestamp: self.timestamp.clone(),
            method: self.method.clone(),
            url: self.url.clone(),
            host: self.host.clone(),
            status: self.status,
            mapped: self.mapped,
            rule_id: self.rule_id.clone(),
            rule_name: self.rule_name.clone(),
            error: self.error.clone(),
            duration_ms: self.finished.unwrap_or_else(|| self.started.elapsed().as_millis() as u64),
            kind: self.kind.clone(),
            request_headers: self.request_headers.clone(),
            response_headers: self.response_headers.clone(),
            request_body_preview,
            response_body_preview,
            request_truncated,
            response_truncated,
        }
    }
}

fn redact_headers(headers: &HeaderMap, redact: bool) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let name = name.to_string();
            let value = if redact && is_sensitive(&name) {
                "[redacted]".to_string()
            } else {
                value.to_str().unwrap_or("").to_string()
            };
            (name, value)
        })
        .collect()
}

fn is_sensitive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    #[test]
    fn redacts_sensitive_headers_and_caps_the_ring() {
        let collector = TrafficCollector::new(true, PREVIEW_BYTES);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        headers.insert("cookie", HeaderValue::from_static("sid=1"));
        headers.insert("x-keep", HeaderValue::from_static("yes"));
        let opened = collector.open("GET", "http://example.test/", "example.test", "http");
        collector.set_headers(opened.id, &headers, true, Some("rule".into()), Some("payments".into()));
        opened.request_preview.append(b"hello");
        let mut response_headers = HeaderMap::new();
        response_headers.insert("set-cookie", HeaderValue::from_static("sid=1"));
        collector.finish(opened.id, Some(200), &response_headers, None);
        let events = collector.list();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_headers.iter().find(|(name, _)| name == "authorization").unwrap().1, "[redacted]");
        assert_eq!(events[0].request_headers.iter().find(|(name, _)| name == "cookie").unwrap().1, "[redacted]");
        assert_eq!(events[0].response_headers.iter().find(|(name, _)| name == "set-cookie").unwrap().1, "[redacted]");
        assert_eq!(events[0].request_headers.iter().find(|(name, _)| name == "x-keep").unwrap().1, "yes");
        assert_eq!(events[0].request_body_preview, "hello");

        let collector = TrafficCollector::new(false, 8);
        for index in 0..RING_LIMIT + 5 {
            let opened = collector.open("GET", &format!("http://example.test/{index}"), "example.test", "http");
            opened.request_preview.append(b"0123456789");
            collector.finish(opened.id, Some(204), &HeaderMap::new(), None);
        }
        let events = collector.list();
        assert_eq!(events.len(), RING_LIMIT);
        let preview = &events[0].request_body_preview;
        assert_eq!(preview.len(), 8);
        assert!(events[0].request_truncated);
    }
}
