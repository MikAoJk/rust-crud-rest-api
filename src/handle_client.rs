use crate::database::DatabasePool;
use flate2::{Compression, write::GzEncoder};
use postgres::Client;
use serde_derive::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::net::TcpStream;

const MIN_COMPRESSION_SIZE: usize = 1024;
const OK_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
const NOT_FOUND: &str = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
const INTERNAL_ERROR: &str = "HTTP/1.1 500 INTERNAL ERROR\r\n\r\n";

#[derive(Serialize, Deserialize)]
struct User {
    id: Option<i32>,
    name: String,
    email: String,
}

type Response = (String, String);
type Handler = fn(&str, &mut Client) -> Result<Response, Box<dyn std::error::Error>>;

pub fn handle_client(mut stream: TcpStream, pool: &DatabasePool) {
    let mut buffer = [0; 1024];

    match stream.read(&mut buffer) {
        Ok(size) => {
            let request = String::from_utf8_lossy(&buffer[..size]);
            let response = handle_request(&request, pool);
            if let Err(error) = write_response(&mut stream, &request, response) {
                eprintln!("Unable to write response: {error}");
            }
        }
        Err(error) => eprintln!("Unable to read stream: {error}"),
    }
}

fn accepts_gzip(request: &str) -> bool {
    let mut gzip = None;
    let mut wildcard = false;
    for line in request
        .split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
    {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("Accept-Encoding") {
            continue;
        }
        for encoding in value.split(',') {
            let mut parts = encoding.split(';');
            let coding = parts.next().unwrap_or_default().trim();
            let mut quality = 1.0;
            for parameter in parts {
                if let Some((name, value)) = parameter.split_once('=')
                    && name.trim().eq_ignore_ascii_case("q")
                {
                    quality = value.trim().parse::<f32>().unwrap_or(0.0);
                }
            }
            let accepted = quality > 0.0 && quality <= 1.0;
            if coding.eq_ignore_ascii_case("gzip") {
                gzip = Some(accepted);
            } else if coding == "*" {
                wildcard = accepted;
            }
        }
    }
    gzip.unwrap_or(wildcard)
}

fn write_response(
    writer: &mut impl Write,
    request: &str,
    (headers, content): Response,
) -> io::Result<()> {
    let mut body = content.into_bytes();
    let mut content_encoding = "";
    if body.len() >= MIN_COMPRESSION_SIZE && accepts_gzip(request) {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&body)?;
        let compressed = encoder.finish()?;
        if compressed.len() < body.len() {
            body = compressed;
            content_encoding = "Content-Encoding: gzip\r\n";
        }
    }

    let headers = format!(
        "{}\r\n{content_encoding}Vary: Accept-Encoding\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        headers.trim_end_matches("\r\n"),
        body.len()
    );
    writer.write_all(headers.as_bytes())?;
    writer.write_all(&body)
}

fn handle_request(request: &str, pool: &DatabasePool) -> Response {
    if request.lines().next().is_some_and(|line| {
        let mut parts = line.split_whitespace();
        parts.next() == Some("GET") && parts.next() == Some("/metrics")
    }) {
        return handle_metrics_request().unwrap_or_else(error_response);
    }

    let handler: Handler = match request {
        r if r.starts_with("POST /users") => handle_post_request,
        r if r.starts_with("GET /users/") => handle_get_request,
        r if r.starts_with("GET /users") => handle_get_all_request,
        r if r.starts_with("PUT /users/") => handle_put_request,
        r if r.starts_with("DELETE /users/") => handle_delete_request,
        _ => return (NOT_FOUND.to_string(), "404 not found".to_string()),
    };

    let result = (|| {
        let mut client = pool.get()?;
        handler(request, &mut client)
    })();

    result.unwrap_or_else(error_response)
}

fn error_response(error: Box<dyn std::error::Error>) -> Response {
    eprintln!("Unable to handle request: {error}");
    (INTERNAL_ERROR.to_string(), "Internal error".to_string())
}

fn handle_metrics_request() -> Result<Response, Box<dyn std::error::Error>> {
    let metrics = crate::metrics::collect()?;
    Ok((
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n\r\n"
            .to_string(),
        serde_json::to_string(&metrics)?,
    ))
}

fn get_id(request: &str) -> &str {
    request
        .split('/')
        .nth(2)
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
}

fn get_user_request_body(request: &str) -> Result<User, serde_json::Error> {
    serde_json::from_str(request.split("\r\n\r\n").last().unwrap_or_default())
}

fn handle_post_request(
    request: &str,
    client: &mut Client,
) -> Result<Response, Box<dyn std::error::Error>> {
    let user = get_user_request_body(request)?;
    let row = client.query_one(
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id",
        &[&user.name, &user.email],
    )?;
    let id: i32 = row.get(0);
    Ok((
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nLocation: /users/{id}\r\n\r\n"
        ),
        "User created".to_string(),
    ))
}

fn handle_get_request(
    request: &str,
    client: &mut Client,
) -> Result<Response, Box<dyn std::error::Error>> {
    let id = get_id(request).parse::<i32>()?;
    match client.query_opt("SELECT id, name, email FROM users WHERE id = $1", &[&id])? {
        Some(row) => {
            let user = User {
                id: row.get(0),
                name: row.get(1),
                email: row.get(2),
            };
            Ok((OK_RESPONSE.to_string(), serde_json::to_string(&user)?))
        }
        None => Ok((NOT_FOUND.to_string(), "User not found".to_string())),
    }
}

fn handle_get_all_request(
    _request: &str,
    client: &mut Client,
) -> Result<Response, Box<dyn std::error::Error>> {
    let mut users = Vec::new();
    for row in client.query("SELECT id, name, email FROM users", &[])? {
        users.push(User {
            id: row.get(0),
            name: row.get(1),
            email: row.get(2),
        });
    }
    Ok((OK_RESPONSE.to_string(), serde_json::to_string(&users)?))
}

fn handle_put_request(
    request: &str,
    client: &mut Client,
) -> Result<Response, Box<dyn std::error::Error>> {
    let id = get_id(request).parse::<i32>()?;
    let user = get_user_request_body(request)?;
    client.execute(
        "UPDATE users SET name = $1, email = $2 WHERE id = $3",
        &[&user.name, &user.email, &id],
    )?;
    Ok((OK_RESPONSE.to_string(), "User updated".to_string()))
}

fn handle_delete_request(
    request: &str,
    client: &mut Client,
) -> Result<Response, Box<dyn std::error::Error>> {
    let id = get_id(request).parse::<i32>()?;
    let rows_affected = client.execute("DELETE FROM users WHERE id = $1", &[&id])?;
    if rows_affected == 0 {
        return Ok((NOT_FOUND.to_string(), "User not found".to_string()));
    }
    Ok((OK_RESPONSE.to_string(), "User deleted".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;

    #[test]
    fn metrics_route_returns_process_metrics_without_a_database_connection() {
        let manager = r2d2_postgres::PostgresConnectionManager::new(
            "host=127.0.0.1 port=1 user=unused".parse().unwrap(),
            postgres::NoTls,
        );
        let pool = DatabasePool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .build_unchecked(manager);
        let request = "GET /metrics HTTP/1.1\r\n\r\n";
        let response = handle_request(request, &pool);
        let mut wire = Vec::new();
        write_response(&mut wire, request, response).unwrap();
        let (headers, body) = response_parts(&wire);
        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("\r\nContent-Type: application/json"));
        assert!(headers.contains("\r\nCache-Control: no-store"));
        let metrics: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(metrics["process_id"], std::process::id());
        assert!(metrics["memory_rss_bytes"].as_u64().unwrap() > 0);
        assert!(metrics["cpu_seconds_total"].as_f64().unwrap() >= 0.0);
        assert!(metrics["sampled_at_unix_seconds"].as_f64().unwrap() > 0.0);

        for request in [
            "GET /metrics-extra HTTP/1.1\r\n\r\n",
            "GET /metrics/ HTTP/1.1\r\n\r\n",
            "POST /metrics HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(handle_request(request, &pool).0, NOT_FOUND);
        }
    }

    fn response_parts(response: &[u8]) -> (&str, &[u8]) {
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&response[..boundary]).unwrap();
        let body = &response[boundary + 4..];
        assert!(headers.contains(&format!("\r\nContent-Length: {}", body.len())));
        assert!(headers.contains("\r\nVary: Accept-Encoding"));
        assert!(headers.contains("\r\nConnection: close"));
        (headers, body)
    }

    #[test]
    fn gzip_negotiation_respects_client_preferences() {
        for (headers, expected) in [
            ("", false),
            ("Accept-Encoding: br, deflate", false),
            ("Accept-Encoding: gzip", true),
            ("accept-encoding: GZIP; Q=0.5", true),
            ("Accept-Encoding: gzip;q=0", false),
            ("Accept-Encoding: gzip;q=0, *;q=1", false),
            ("Accept-Encoding: *;q=0, gzip;q=1", true),
            ("Accept-Encoding: *", true),
            ("Accept-Encoding: gzip;q=invalid", false),
            ("Accept-Encoding: gzip;q=2", false),
            ("Accept-Encoding: gzip;q=-1", false),
            ("Accept-Encoding: gzip;q=NaN", false),
            ("Accept-Encoding: br\r\nAccept-Encoding: gzip", true),
            ("X-Accept-Encoding: gzip", false),
            ("\r\nAccept-Encoding: gzip", false),
        ] {
            let request = format!("GET /users HTTP/1.1\r\n{headers}\r\n\r\n");
            assert_eq!(accepts_gzip(&request), expected, "{headers:?}");
        }
    }

    #[test]
    fn large_user_list_is_smaller_on_the_wire_without_losing_data() {
        let users: Vec<User> = (1..=200)
            .map(|id| User {
                id: Some(id),
                name: format!("k6-iteration-{id}"),
                email: format!("k6-iteration-{id}@example.com"),
            })
            .collect();
        let content = serde_json::to_string(&users).unwrap();
        let request = "GET /users HTTP/1.1\r\nAccept-Encoding: gzip\r\n\r\n";
        let mut response = Vec::new();
        write_response(
            &mut response,
            request,
            (OK_RESPONSE.to_string(), content.clone()),
        )
        .unwrap();

        let (headers, body) = response_parts(&response);
        assert!(headers.contains("\r\nContent-Encoding: gzip"));
        let mut decoded = String::new();
        GzDecoder::new(body).read_to_string(&mut decoded).unwrap();
        assert_eq!(decoded, content);

        let mut uncompressed = Vec::new();
        write_response(
            &mut uncompressed,
            "GET /users HTTP/1.1\r\n\r\n",
            (OK_RESPONSE.to_string(), content),
        )
        .unwrap();
        assert!(
            response.len() * 4 < uncompressed.len(),
            "Expected at least a 4x wire-size reduction: {} vs {} bytes",
            response.len(),
            uncompressed.len()
        );
    }

    #[test]
    fn responses_stay_uncompressed_without_gzip_support() {
        let content = "user data ".repeat(200);
        for encoding in ["", "Accept-Encoding: br", "Accept-Encoding: gzip;q=0"] {
            let mut response = Vec::new();
            write_response(
                &mut response,
                &format!("GET /users HTTP/1.1\r\n{encoding}\r\n\r\n"),
                (OK_RESPONSE.to_string(), content.clone()),
            )
            .unwrap();
            let (headers, body) = response_parts(&response);
            assert!(!headers.contains("Content-Encoding:"));
            assert_eq!(body, content.as_bytes());
        }
    }

    #[test]
    fn compression_starts_at_one_kibibyte() {
        for size in [MIN_COMPRESSION_SIZE - 1, MIN_COMPRESSION_SIZE] {
            let content = "a".repeat(size);
            let mut response = Vec::new();
            write_response(
                &mut response,
                "GET /users HTTP/1.1\r\nAccept-Encoding: gzip\r\n\r\n",
                (OK_RESPONSE.to_string(), content.clone()),
            )
            .unwrap();
            let (headers, body) = response_parts(&response);
            if size < MIN_COMPRESSION_SIZE {
                assert!(!headers.contains("Content-Encoding:"));
                assert_eq!(body, content.as_bytes());
            } else {
                assert!(headers.contains("\r\nContent-Encoding: gzip"));
                let mut decoded = String::new();
                GzDecoder::new(body).read_to_string(&mut decoded).unwrap();
                assert_eq!(decoded, content);
            }
        }
    }

    #[test]
    fn small_responses_preserve_status_location_and_body() {
        for (headers, content) in [
            (OK_RESPONSE, "User updated"),
            (OK_RESPONSE, "User deleted"),
            (OK_RESPONSE, "[]"),
            (NOT_FOUND, "User not found"),
            (INTERNAL_ERROR, "Internal error"),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nLocation: /users/42\r\n\r\n",
                "User created",
            ),
            (OK_RESPONSE, "na\u{ef}ve"),
            (OK_RESPONSE, ""),
        ] {
            let mut response = Vec::new();
            write_response(
                &mut response,
                "GET /users HTTP/1.1\r\nAccept-Encoding: gzip\r\n\r\n",
                (headers.to_string(), content.to_string()),
            )
            .unwrap();
            let (actual_headers, body) = response_parts(&response);
            assert!(actual_headers.starts_with(headers.trim_end_matches("\r\n")));
            assert!(!actual_headers.contains("Content-Encoding:"));
            assert_eq!(body, content.as_bytes());
        }
    }

    #[test]
    fn response_write_errors_are_propagated() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "connection closed",
                ))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let error = write_response(
            &mut BrokenWriter,
            "GET /users HTTP/1.1\r\n\r\n",
            (OK_RESPONSE.to_string(), "[]".to_string()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
