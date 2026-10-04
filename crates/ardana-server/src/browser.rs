//! The browser variants of library models, for the playground's in-tab engine: `GET /v1/browser/<name>/<file>` serves
//! `model.onnx`, `model.onnx.data` and `tokenizer.json` from the server's hub cache, and `profile`, the
//! [`ardana_core::ModelProfile`] the registry derives for them, as JSON. Every other name and file is a 404.
//!
//! The first request for a model pulls its variant ([`ardana_registry::pull_browser`]: the hub cache first, then the
//! Hub); requests for the same model share that pull, and a pulled variant is kept while the server runs. Files go
//! out as stored, never compressed, with their `Content-Length`, so a tab can show how far its download is, and by
//! byte range (`Range`, `If-Range`, 206), so a tab resumes a download that stopped; every response names the snapshot
//! commit as its `ETag` and keeps out of the HTTP cache, since the tab keeps the files itself.

use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use ardana_registry::library::LibraryEntry;
use ardana_registry::{BROWSER_FILES, BrowserModel, TOKENIZER_FILE};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::io::{AsyncRead, AsyncSeekExt, ReadBuf};

use crate::error::ApiError;
use crate::models::Models;

/// The route's file name for the profile.
const PROFILE: &str = "profile";
/// How much of a file one body frame carries.
const CHUNK: usize = 1 << 20;

/// `GET /v1/browser/<name>/<file>`.
pub(crate) async fn file(
    State(models): State<Arc<Models>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Ok(Path((name, file))) = path else {
        return Err(ApiError::NotFound);
    };
    if models.library().await.browser(&name).is_none()
        || (file != PROFILE && !BROWSER_FILES.contains(&file.as_str()))
    {
        return Err(ApiError::NotFound);
    }
    serve(&models, &name, &file, &headers).await
}

/// `file` of the browser variant of `name`, which the first request pulls: whole, or the one byte range `headers` ask
/// for.
async fn serve(
    models: &Models,
    name: &str,
    file: &str,
    headers: &HeaderMap,
) -> Result<Response, ApiError> {
    let model = models.browser(name).await?;
    let version = format!("\"{}\"", model.commit);
    let etag = HeaderValue::from_str(&version)
        .map_err(|err| ApiError::Internal(format!("the commit of {name}: {err}")))?;
    let kept = [
        (header::ETAG, etag),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
    ];
    if file == PROFILE {
        return Ok((kept, Json(&model.profile)).into_response());
    }
    let Some(path) = model.file(file) else {
        return Err(ApiError::NotFound);
    };
    let mut opened = match tokio::fs::File::open(path).await {
        Ok(opened) => opened,
        Err(err) => {
            // The cache lost the file: the next request pulls the variant again.
            models.forget_browser(name);
            return Err(ApiError::Internal(format!(
                "the browser file {file} of {name} is missing from the Hugging Face cache: {err}"
            )));
        }
    };
    let len = opened
        .metadata()
        .await
        .map_err(|err| {
            ApiError::Internal(format!("reading the browser file {file} of {name}: {err}"))
        })?
        .len();
    let content_type = if file == TOKENIZER_FILE {
        "application/json"
    } else {
        "application/octet-stream"
    };
    let described = [
        (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
        (header::ACCEPT_RANGES, HeaderValue::from_static("bytes")),
    ];
    let (start, end) = match part(headers, &version, len) {
        Part::Whole => {
            let length = [(header::CONTENT_LENGTH, HeaderValue::from(len))];
            let body = Body::new(FileBody::new(opened, len));
            return Ok((kept, described, length, body).into_response());
        }
        Part::Range(start, end) => (start, end),
        Part::Unsatisfiable => return Err(ApiError::RangeNotSatisfiable(len)),
    };
    opened.seek(SeekFrom::Start(start)).await.map_err(|err| {
        ApiError::Internal(format!("reading the browser file {file} of {name}: {err}"))
    })?;
    let part_len = end - start + 1;
    let range = HeaderValue::from_str(&format!("bytes {start}-{end}/{len}"))
        .map_err(|err| ApiError::Internal(format!("the range of {file}: {err}")))?;
    let length = [
        (header::CONTENT_LENGTH, HeaderValue::from(part_len)),
        (header::CONTENT_RANGE, range),
    ];
    let body = Body::new(FileBody::new(opened, part_len));
    Ok((StatusCode::PARTIAL_CONTENT, kept, described, length, body).into_response())
}

/// What a request asks of a file: `Range` with one byte range (RFC 9110), honoured unless `If-Range` names another
/// version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The whole file: no range, one this server answers whole (several ranges, another unit, one that does not
    /// parse), or an `If-Range` that is not the file's `ETag` (no file here has a `Last-Modified` for a date to match).
    Whole,
    /// The bytes from the first to the last, both included.
    Range(u64, u64),
    /// A range that starts at or past the end of the file.
    Unsatisfiable,
}

/// The part of a file of `len` bytes, whose `ETag` is `etag`, that `headers` ask for.
fn part(headers: &HeaderMap, etag: &str, len: u64) -> Part {
    let Some(range) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) else {
        return Part::Whole;
    };
    if headers
        .get(header::IF_RANGE)
        .is_some_and(|version| version.as_bytes() != etag.as_bytes())
    {
        return Part::Whole;
    }
    // A comma (several ranges) leaves a bound that does not parse: the whole file.
    let Some((first, last)) = range
        .trim()
        .strip_prefix("bytes=")
        .and_then(|spec| spec.split_once('-'))
    else {
        return Part::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        // The last `suffix` bytes.
        return match last.parse::<u64>() {
            Ok(0) => Part::Unsatisfiable,
            Ok(_) if len == 0 => Part::Unsatisfiable,
            Ok(suffix) => Part::Range(len - suffix.min(len), len - 1),
            Err(_) => Part::Whole,
        };
    }
    let Ok(first) = first.parse::<u64>() else {
        return Part::Whole;
    };
    let last = match last {
        "" => None,
        last => match last.parse::<u64>() {
            Ok(last) if last >= first => Some(last),
            _ => return Part::Whole,
        },
    };
    if first >= len {
        return Part::Unsatisfiable;
    }
    Part::Range(first, last.map_or(len - 1, |last| last.min(len - 1)))
}

/// Pulls the browser variant of the library model `model`, logging what it pulled or why it could not; the server's
/// [`crate::models::Pulls`] shares it among the requests for the model and keeps what it pulled.
pub(crate) async fn pull(model: LibraryEntry) -> Result<Arc<BrowserModel>, String> {
    let name = model.name.clone();
    if let Some(browser) = &model.browser {
        eprintln!(
            "ardana serve: pulling the browser variant of {name} ({}, {})",
            browser.weights,
            ardana_api::human_size(browser.size)
        );
    }
    let handle = tokio::runtime::Handle::current();
    // The pull reads the tokenizer and the chat template synchronously: off the async workers.
    let outcome = tokio::task::spawn_blocking(move || {
        handle.block_on(ardana_registry::pull_browser(&model, true))
    })
    .await
    .map_err(|err| format!("pulling the browser variant of {name} stopped: {err}"))
    .and_then(|pulled| {
        pulled.map_err(|err| format!("pulling the browser variant of {name}: {err}"))
    });
    match &outcome {
        Ok(model) => eprintln!(
            "ardana serve: serving the browser variant of {name} ({} at {})",
            model.reference, model.commit
        ),
        Err(err) => eprintln!("ardana serve: {err}"),
    }
    outcome.map(Arc::new)
}

/// A file's bytes as a response body of known length, read as the client takes them.
struct FileBody {
    file: tokio::fs::File,
    /// Bytes still to send.
    left: u64,
    buf: Box<[u8]>,
}

impl FileBody {
    fn new(file: tokio::fs::File, len: u64) -> FileBody {
        let size = usize::try_from(len).map_or(CHUNK, |len| len.min(CHUNK));
        FileBody {
            file,
            left: len,
            buf: vec![0; size].into_boxed_slice(),
        }
    }
}

impl HttpBody for FileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = self.get_mut();
        if this.left == 0 {
            return Poll::Ready(None);
        }
        let want =
            usize::try_from(this.left).map_or(this.buf.len(), |left| left.min(this.buf.len()));
        let mut read = ReadBuf::new(&mut this.buf[..want]);
        ready!(Pin::new(&mut this.file).poll_read(cx, &mut read))?;
        let chunk = read.filled();
        if chunk.is_empty() {
            return Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the file is shorter than when it was opened",
            ))));
        }
        this.left -= chunk.len() as u64;
        Poll::Ready(Some(Ok(Frame::data(Bytes::copy_from_slice(chunk)))))
    }

    fn is_end_stream(&self) -> bool {
        self.left == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part a request with `range` and `if_range` asks of a file of 100 bytes whose `ETag` is `"abc"`.
    fn asked(range: Option<&str>, if_range: Option<&str>) -> Part {
        let mut headers = HeaderMap::new();
        for (name, value) in [(header::RANGE, range), (header::IF_RANGE, if_range)] {
            if let Some(value) = value {
                headers.insert(name, HeaderValue::from_str(value).expect("a header value"));
            }
        }
        part(&headers, "\"abc\"", 100)
    }

    /// One byte range is its part; a range past the end is unsatisfiable; anything else is the whole file, as is a
    /// range on another version.
    #[test]
    fn ranges_name_one_part_of_this_version() {
        for (range, part) in [
            ("bytes=0-15", Part::Range(0, 15)),
            ("bytes=90-", Part::Range(90, 99)),
            ("bytes=90-200", Part::Range(90, 99)),
            ("bytes=99-99", Part::Range(99, 99)),
            ("bytes=-10", Part::Range(90, 99)),
            ("bytes=-500", Part::Range(0, 99)),
            (" bytes= 10 - 20 ", Part::Range(10, 20)),
            ("bytes=100-", Part::Unsatisfiable),
            ("bytes=150-160", Part::Unsatisfiable),
            ("bytes=-0", Part::Unsatisfiable),
            ("bytes=0-1,5-6", Part::Whole),
            ("bytes=5-1", Part::Whole),
            ("bytes=a-", Part::Whole),
            ("bytes=-", Part::Whole),
            ("bytes=", Part::Whole),
            ("items=0-15", Part::Whole),
            ("0-15", Part::Whole),
        ] {
            assert_eq!(asked(Some(range), None), part, "{range:?}");
        }
        assert_eq!(asked(None, None), Part::Whole);
        assert_eq!(asked(None, Some("\"abc\"")), Part::Whole);
        assert_eq!(
            asked(Some("bytes=10-"), Some("\"abc\"")),
            Part::Range(10, 99)
        );
        for other in ["\"abd\"", "W/\"abc\"", "Sat, 03 Oct 2026 10:00:00 GMT"] {
            assert_eq!(
                asked(Some("bytes=10-"), Some(other)),
                Part::Whole,
                "{other}"
            );
        }
    }
}
