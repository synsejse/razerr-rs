use std::{
    error::Error,
    fs,
    io::Cursor,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use image::{ImageBuffer, ImageFormat, Rgb};
use razerr_images::{ImageLoader, MAX_BYTES, decode};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::{JoinHandle, JoinSet},
};

const PNG: &[u8] = include_bytes!("data/picture.png");
type TestResult = Result<(), Box<dyn Error>>;

struct HttpFixture {
    base: String,
    requests: Arc<AtomicUsize>,
    slow_started: Arc<Notify>,
    slow_closed: Arc<Notify>,
    task: Option<JoinHandle<()>>,
}

impl HttpFixture {
    async fn start() -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let slow_started = Arc::new(Notify::new());
        let slow_closed = Arc::new(Notify::new());
        let count = requests.clone();
        let started = slow_started.clone();
        let closed = slow_closed.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let count = count.clone();
                        let started = started.clone();
                        let closed = closed.clone();
                        connections.spawn(async move {
                            let _ = serve(stream, &count, &started, &closed).await;
                        });
                    }
                    Some(_) = connections.join_next() => {}
                }
            }
        });
        Ok(Self {
            base,
            requests,
            slow_started,
            slow_closed,
            task: Some(task),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn stop(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn serve(
    mut stream: TcpStream,
    requests: &AtomicUsize,
    started: &Notify,
    closed: &Notify,
) -> std::io::Result<()> {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut bytes = [0; 1024];
        let length = stream.read(&mut bytes).await?;
        if length == 0 || request.len() + length > 8192 {
            return Ok(());
        }
        request.extend_from_slice(&bytes[..length]);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_ascii_whitespace().nth(1).unwrap_or("");
    requests.fetch_add(1, Ordering::SeqCst);
    match path {
        "/slow" => {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 74\r\nConnection: close\r\n\r\n")
                .await?;
            started.notify_one();
            let mut byte = [0];
            let _ = stream.read(&mut byte).await;
            closed.notify_one();
        }
        "/large" => {
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        MAX_BYTES + 1
                    )
                    .as_bytes(),
                )
                .await?;
        }
        "/chunked" => {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await?;
            let chunk = vec![0; 64 * 1024];
            for _ in 0..=MAX_BYTES / chunk.len() {
                stream.write_all(b"10000\r\n").await?;
                stream.write_all(&chunk).await?;
                stream.write_all(b"\r\n").await?;
            }
            stream.write_all(b"0\r\n\r\n").await?;
        }
        "/redirect" => {
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
        }
        "/loop" => {
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
        }
        "/unsafe" => {
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: file:///etc/passwd\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
        }
        _ => {
            let (status, body) = match path {
                "/image" => ("200 OK", PNG),
                "/bad" => ("200 OK", b"not an image".as_slice()),
                _ => ("404 Not Found", b"missing".as_slice()),
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await?;
            stream.write_all(body).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn local_images_and_source_validation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let loader = ImageLoader::new(directory.path().join("cache"))?;
    let source = directory.path().join("image.png");
    fs::write(&source, PNG)?;
    let image = loader.load(source.to_str().ok_or("path")?).await?;
    assert_eq!((image.width, image.height, image.pixels.len()), (4, 2, 32));
    assert!(!directory.path().join("cache").exists());
    for source in [
        "",
        "relative.png",
        "file:///etc/passwd",
        "ftp://example.org/image.png",
        "data:image/png,foo",
    ] {
        assert!(loader.load(source).await.is_err());
    }
    assert!(
        loader
            .load(directory.path().to_str().ok_or("path")?)
            .await
            .is_err()
    );
    let oversized = directory.path().join("oversized");
    fs::File::create(&oversized)?.set_len(MAX_BYTES as u64 + 1)?;
    assert!(matches!(
        loader.load(oversized.to_str().ok_or("path")?).await,
        Err(razerr_images::Error::TooLarge)
    ));
    assert!(ImageLoader::new("relative-cache").is_err());
    Ok(())
}

#[test]
fn png_jpeg_and_corrupt_input() -> TestResult {
    assert_eq!(decode(PNG)?.pixels.len(), 32);
    let rgb = ImageBuffer::from_pixel(4, 2, Rgb([10u8, 20, 30]));
    let mut bytes = Cursor::new(Vec::new());
    rgb.write_to(&mut bytes, ImageFormat::Jpeg)?;
    let image = decode(bytes.get_ref())?;
    assert_eq!((image.width, image.height), (4, 2));
    for pixel in image.pixels.as_chunks::<4>().0 {
        assert!(
            pixel[0].abs_diff(10) <= 5 && pixel[1].abs_diff(20) <= 5 && pixel[2].abs_diff(30) <= 5
        );
        assert_eq!(pixel[3], 255);
    }
    assert!(decode(&bytes.get_ref()[..bytes.get_ref().len() / 2]).is_err());
    assert!(decode(&PNG[..PNG.len() / 2]).is_err());
    assert!(decode(b"not an image").is_err());
    // Even if another workspace dependency enables GIF support, this loader must reject it.
    assert!(matches!(
        decode(b"GIF89a"),
        Err(razerr_images::Error::UnsupportedFormat)
    ));
    Ok(())
}

#[tokio::test]
async fn cache_repairs_corruption_and_works_offline() -> TestResult {
    let server = HttpFixture::start().await?;
    let directory = tempfile::tempdir()?;
    let loader = ImageLoader::new(directory.path().join("cache"))?;
    let url = server.url("/image");
    let expected = loader.load(&url).await?;
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(loader.load(&url).await?, expected);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    let cached = loader.cache_path(&url);
    assert_eq!(fs::metadata(&cached)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(directory.path().join("cache"))?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    fs::write(&cached, "corrupt")?;
    assert_eq!(loader.load(&url).await?, expected);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    assert_eq!(fs::read_dir(directory.path().join("cache"))?.count(), 1);
    server.stop().await;
    assert_eq!(loader.load(&url).await?, expected);
    Ok(())
}

#[tokio::test]
async fn response_limits_redirects_and_invalid_responses() -> TestResult {
    let server = HttpFixture::start().await?;
    let directory = tempfile::tempdir()?;
    let loader = ImageLoader::new(directory.path().join("cache"))?;
    assert_eq!(loader.load(&server.url("/redirect")).await?.width, 4);
    for path in ["/large", "/chunked", "/bad", "/missing", "/unsafe", "/loop"] {
        let url = server.url(path);
        assert!(loader.load(&url).await.is_err(), "accepted {path}");
        assert!(!loader.cache_path(&url).exists(), "cached {path}");
    }
    Ok(())
}

#[tokio::test]
async fn dropping_a_download_closes_the_stalled_connection() -> TestResult {
    let server = HttpFixture::start().await?;
    let directory = tempfile::tempdir()?;
    let loader = ImageLoader::new(directory.path().join("cache"))?;
    let url = server.url("/slow");
    let task_loader = loader.clone();
    let task_url = url.clone();
    let task = tokio::spawn(async move { task_loader.load(&task_url).await });
    tokio::time::timeout(Duration::from_secs(2), server.slow_started.notified()).await?;
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(2), server.slow_closed.notified()).await?;
    assert!(!loader.cache_path(&url).exists());
    Ok(())
}
