use crate::handle_client::{CLIENT_IO_TIMEOUT, is_metrics_request};
use std::io;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

const WORKER_COUNT: usize = 64;
const METRICS_WORKER_COUNT: usize = 2;
const MAX_IN_FLIGHT_CONNECTIONS: usize = 256;
const CONNECTION_BACKLOG: i32 = 1024;
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_REQUEST_SIZE: usize = 64 * 1024;

pub fn serve_connections(
    listener: TcpListener,
    handler: impl Fn(&str) -> io::Result<Vec<u8>> + Send + Sync + 'static,
) -> io::Result<()> {
    // Queue connection bursts in the kernel without opening more client descriptors.
    socket2::SockRef::from(&listener).listen(CONNECTION_BACKLOG)?;
    listener.set_nonblocking(true)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(WORKER_COUNT + METRICS_WORKER_COUNT)
        .build()?
        .block_on(async { serve(tokio::net::TcpListener::from_std(listener)?, handler).await })
}

async fn serve(
    listener: tokio::net::TcpListener,
    handler: impl Fn(&str) -> io::Result<Vec<u8>> + Send + Sync + 'static,
) -> io::Result<()> {
    let handler = Arc::new(handler);
    let workers = Arc::new(Semaphore::new(WORKER_COUNT));
    let metrics_workers = Arc::new(Semaphore::new(METRICS_WORKER_COUNT));
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept(), if connections.len() < MAX_IN_FLIGHT_CONNECTIONS => {
                let (stream, _) = match accepted {
                    Ok(connection) => connection,
                    // Unix EMFILE/ENFILE can clear as existing connections finish.
                    Err(error) if cfg!(unix) && matches!(error.raw_os_error(), Some(23 | 24)) => {
                        eprintln!("Unable to accept connection; retrying in {ACCEPT_RETRY_DELAY:?}: {error}");
                        tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let handler = Arc::clone(&handler);
                let workers = Arc::clone(&workers);
                let metrics_workers = Arc::clone(&metrics_workers);
                connections.spawn(async move {
                    handle_connection(stream, handler, workers, metrics_workers).await
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                match completed {
                    Some(Ok(Ok(()))) => {}
                    Some(Ok(Err(error))) => eprintln!("Unable to handle connection: {error}"),
                    Some(Err(error)) => eprintln!("Connection task failed: {error}"),
                    None => {}
                }
            }
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    handler: Arc<impl Fn(&str) -> io::Result<Vec<u8>> + Send + Sync + 'static>,
    workers: Arc<Semaphore>,
    metrics_workers: Arc<Semaphore>,
) -> io::Result<()> {
    let request = match timeout(CLIENT_IO_TIMEOUT, read_request(&mut stream)).await? {
        Ok(request) => request,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            let response =
                b"HTTP/1.1 400 BAD REQUEST\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            timeout(CLIENT_IO_TIMEOUT, stream.write_all(response)).await??;
            return Err(error);
        }
        Err(error) => return Err(error),
    };

    // Reserve execution capacity for metrics even when every CRUD worker is blocked.
    let workers = if is_metrics_request(&request) {
        metrics_workers
    } else {
        workers
    };
    let permit = workers.acquire_owned().await.map_err(io::Error::other)?;
    let response = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        handler(&request)
    })
    .await
    .map_err(io::Error::other)??;

    timeout(CLIENT_IO_TIMEOUT, stream.write_all(&response)).await??;
    timeout(CLIENT_IO_TIMEOUT, stream.shutdown()).await??;
    Ok(())
}

async fn read_request(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<String> {
    let mut request = Vec::new();
    let mut expected_size = None;
    let mut buffer = [0; 1024];

    loop {
        let size = reader.read(&mut buffer).await?;
        if size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Connection closed before the request was complete",
            ));
        }
        request.extend_from_slice(&buffer[..size]);
        if request.len() > MAX_REQUEST_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Request exceeds 64 KiB",
            ));
        }

        if expected_size.is_none()
            && let Some(boundary) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..boundary])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let mut content_length = None;
            for line in headers.split("\r\n").skip(1) {
                let (name, value) = line.split_once(':').ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid request header")
                })?;
                if name.eq_ignore_ascii_case("Transfer-Encoding") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Transfer-Encoding is not supported",
                    ));
                }
                if name.eq_ignore_ascii_case("Content-Length") {
                    let length = value
                        .trim()
                        .parse::<usize>()
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if content_length.replace(length).is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Duplicate Content-Length",
                        ));
                    }
                }
            }
            let total = (boundary + 4)
                .checked_add(content_length.unwrap_or(0))
                .filter(|size| *size <= MAX_REQUEST_SIZE)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Request exceeds 64 KiB")
                })?;
            expected_size = Some(total);
        }

        if let Some(size) = expected_size
            && request.len() >= size
        {
            request.truncate(size);
            return String::from_utf8(request)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};
    use tokio::sync::mpsc;

    struct ReleaseWorkers(Arc<(Mutex<bool>, Condvar)>);

    impl Drop for ReleaseWorkers {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }

    #[tokio::test]
    async fn connection_limit_backpressures_slow_clients_and_recovers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, |_| {
            Ok(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec())
        }));

        let mut clients = Vec::new();
        for _ in 0..MAX_IN_FLIGHT_CONNECTIONS {
            let mut client = timeout(Duration::from_secs(5), TcpStream::connect(address))
                .await
                .unwrap()
                .unwrap();
            client.write_all(b"GET /met").await.unwrap();
            clients.push(client);
        }

        let mut queued = TcpStream::connect(address).await.unwrap();
        queued
            .write_all(b"GET /metrics HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        assert!(
            timeout(
                Duration::from_millis(100),
                queued.read_to_end(&mut response)
            )
            .await
            .is_err(),
            "Requests beyond the connection limit must wait in the TCP backlog"
        );
        assert!(response.is_empty());
        assert!(!server.is_finished());

        drop(clients.pop());
        timeout(Duration::from_secs(5), queued.read_to_end(&mut response))
            .await
            .expect("A released connection slot must allow the next request through")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(!server.is_finished());

        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn descriptor_exhaustion_does_not_stop_server() {
        const CHILD_ENV: &str = "RUST_CRUD_TEST_DESCRIPTOR_EXHAUSTION";
        const TEST_NAME: &str = "server::tests::descriptor_exhaustion_does_not_stop_server";
        if std::env::var_os(CHILD_ENV).is_none() {
            let output = std::process::Command::new("sh")
                .args(["-c", "ulimit -n 128 && exec \"$@\"", "sh"])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CHILD_ENV, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Descriptor exhaustion subprocess failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("Unable to accept connection; retrying"),
                "The subprocess must exercise the accept-error recovery path"
            );
            return;
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        client
            .write_all(b"GET /metrics HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let server = tokio::spawn(serve(listener, |_| {
            Ok(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec())
        }));

        let mut files = Vec::new();
        loop {
            match std::fs::File::open("/dev/null") {
                Ok(file) => files.push(file),
                Err(error) => {
                    assert_eq!(error.raw_os_error(), Some(24));
                    break;
                }
            }
        }
        assert!(!files.is_empty());
        tokio::time::sleep(ACCEPT_RETRY_DELAY * 3).await;
        assert!(
            !server.is_finished(),
            "EMFILE must not stop the accept loop"
        );

        drop(files);
        let mut response = Vec::new();
        timeout(Duration::from_secs(5), client.read_to_end(&mut response))
            .await
            .expect("The server must recover once descriptors are available")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(!server.is_finished());

        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn metrics_bypass_blocked_crud_workers_queued_requests_and_slow_clients() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = ReleaseWorkers(Arc::clone(&gate));
        let (started_sender, mut started_receiver) = mpsc::unbounded_channel();
        let manager = r2d2_postgres::PostgresConnectionManager::new(
            "host=127.0.0.1 port=1 user=unused".parse().unwrap(),
            postgres::NoTls,
        );
        let pool = crate::database::DatabasePool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .build_unchecked(manager);
        let server = tokio::spawn(serve(listener, move |request| {
            if is_metrics_request(request) {
                return crate::handle_client::handle_client(request, &pool);
            }
            started_sender.send(()).unwrap();
            let (released, changed) = &*gate;
            let mut released = released.lock().unwrap();
            while !*released {
                released = changed.wait(released).unwrap();
            }
            Ok(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec())
        }));

        let mut clients = Vec::new();
        for _ in 0..WORKER_COUNT {
            let mut client = TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET /users HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            clients.push(client);
        }
        for _ in 0..WORKER_COUNT {
            timeout(Duration::from_secs(5), started_receiver.recv())
                .await
                .unwrap()
                .unwrap();
        }
        for _ in 0..(MAX_IN_FLIGHT_CONNECTIONS - 2 * WORKER_COUNT - 1) {
            let mut client = TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET /users HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            clients.push(client);
        }
        for _ in 0..WORKER_COUNT {
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(b"GET /met").await.unwrap();
            clients.push(client);
        }

        let response = timeout(Duration::from_secs(1), async {
            let mut client = TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET /metrics HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            response
        })
        .await
        .expect("Metrics must respond without releasing any CRUD worker");
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let metrics: serde_json::Value = serde_json::from_slice(&response[boundary + 4..]).unwrap();
        assert_eq!(metrics["process_id"], std::process::id());
        assert!(metrics["memory_rss_bytes"].as_u64().unwrap() > 0);

        drop(release);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn fragmented_request_waits_for_headers_and_body() {
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let request = "POST /users HTTP/1.1\r\ncontent-length: 2\r\n\r\n{}";
        let writing = tokio::spawn(async move {
            for byte in request.as_bytes() {
                writer.write_all(&[*byte]).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        assert_eq!(read_request(&mut reader).await.unwrap(), request);
        writing.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_or_oversized_requests_are_rejected() {
        for request in [
            "POST /users HTTP/1.1\r\nContent-Length: invalid\r\n\r\n",
            "POST /users HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            "POST /users HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n",
            "POST /users HTTP/1.1\r\nContent-Length: 65536\r\n\r\n",
            "POST /users HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "GET /metrics HTTP/1.1\r\ninvalid header\r\n\r\n",
        ] {
            let mut reader = request.as_bytes();
            assert_eq!(
                read_request(&mut reader).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{request:?}"
            );
        }
        let request = vec![b'a'; MAX_REQUEST_SIZE + 1];
        assert_eq!(
            read_request(&mut request.as_slice())
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn incomplete_requests_report_unexpected_eof() {
        for request in [
            "",
            "GET /metrics HTTP/1.1\r\n",
            "POST /users HTTP/1.1\r\nContent-Length: 2\r\n\r\n{",
        ] {
            assert_eq!(
                read_request(&mut request.as_bytes())
                    .await
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }
}
