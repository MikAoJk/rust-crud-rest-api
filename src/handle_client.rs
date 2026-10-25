use crate::database::DatabasePool;
use postgres::Client;
use serde_derive::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

const OK_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
const NOT_FOUND: &str = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
const INTERNAL_ERROR: &str = "HTTP/1.1 500 INTERNAL ERROR\r\n\r\n";
const MAX_HEADER_SIZE: usize = 64 * 1024;
const MAX_BODY_SIZE: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct User {
    id: Option<i32>,
    name: String,
    email: String,
}

type Response = (String, String);
type Handler = fn(&str, &mut Client) -> Result<Response, Box<dyn std::error::Error>>;

pub fn handle_client(stream: TcpStream, pool: &DatabasePool) {
    handle_client_with(stream, |request| handle_request(request, pool));
}

fn handle_client_with(stream: TcpStream, mut handler: impl FnMut(&str) -> Response) {
    let mut reader = BufReader::new(stream);
    loop {
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                eprintln!("Unable to read stream: {error}");
                break;
            }
        };
        let keep_alive = request_allows_keep_alive(&request);
        let (status_headers, content) = handler(&request);
        if let Err(error) = write_response(reader.get_mut(), &status_headers, &content, keep_alive)
        {
            eprintln!("Unable to write response: {error}");
            break;
        }
        if !keep_alive {
            break;
        }
    }
}

fn read_request(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut request = Vec::new();
    let mut line = Vec::new();

    if reader.read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    request.extend_from_slice(&line);
    if request.len() > MAX_HEADER_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request headers are too large",
        ));
    }
    line.clear();

    let mut content_length = 0;
    loop {
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request headers were complete",
            ));
        }
        request.extend_from_slice(&line);
        if request.len() > MAX_HEADER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers are too large",
            ));
        }

        if line == b"\r\n" || line == b"\n" {
            break;
        }
        if let Some(separator) = line.iter().position(|byte| *byte == b':') {
            let (name, value) = line.split_at(separator);
            let value = &value[1..];
            if name.eq_ignore_ascii_case(b"content-length") {
                let value = std::str::from_utf8(value)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if value > MAX_BODY_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request body is too large",
                    ));
                }
                content_length = value;
            } else if name.eq_ignore_ascii_case(b"transfer-encoding")
                && !value.trim_ascii().eq_ignore_ascii_case(b"identity")
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "transfer-encoded requests are not supported",
                ));
            }
        }
        line.clear();
    }

    let body_start = request.len();
    request.resize(body_start + content_length, 0);
    reader.read_exact(&mut request[body_start..])?;
    Ok(Some(String::from_utf8_lossy(&request).into_owned()))
}

fn request_allows_keep_alive(request: &str) -> bool {
    let mut lines = request.lines();
    let version = lines.next().and_then(|line| line.split_whitespace().last());
    let mut connection_close = false;
    let mut connection_keep_alive = false;

    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("connection") {
            for token in value.split(',').map(str::trim) {
                connection_close |= token.eq_ignore_ascii_case("close");
                connection_keep_alive |= token.eq_ignore_ascii_case("keep-alive");
            }
        }
    }

    !connection_close
        && (version == Some("HTTP/1.1") || (version == Some("HTTP/1.0") && connection_keep_alive))
}

fn write_response(
    stream: &mut TcpStream,
    status_headers: &str,
    content: &str,
    keep_alive: bool,
) -> io::Result<()> {
    let headers = status_headers.strip_suffix("\r\n\r\n").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "response headers are incomplete",
        )
    })?;
    write!(
        stream,
        "{headers}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{}",
        content.len(),
        if keep_alive { "keep-alive" } else { "close" },
        content
    )
}

fn handle_request(request: &str, pool: &DatabasePool) -> Response {
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

    result.unwrap_or_else(|error| {
        eprintln!("Unable to handle request: {error}");
        (INTERNAL_ERROR.to_string(), "Internal error".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn read_response(reader: &mut impl BufRead) -> (String, String) {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .and_then(|value| value.parse::<usize>().ok())
            })
            .unwrap();
        let mut content = vec![0; content_length];
        reader.read_exact(&mut content).unwrap();
        (headers, String::from_utf8(content).unwrap())
    }

    #[test]
    fn serves_multiple_framed_requests_on_one_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let server_stream = listener.accept().unwrap().0;
        let server = std::thread::spawn(move || {
            handle_client_with(server_stream, |_| {
                (OK_RESPONSE.to_string(), "hello".to_string())
            });
        });

        client
            .write_all(
                b"GET /first HTTP/1.1\r\nHost: localhost\r\n\r\n\
                  GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        let mut reader = BufReader::new(client);
        let (first_headers, first_content) = read_response(&mut reader);
        let (second_headers, second_content) = read_response(&mut reader);
        server.join().unwrap();

        assert!(first_headers.contains("Content-Length: 5\r\n"));
        assert!(first_headers.contains("Connection: keep-alive\r\n"));
        assert!(second_headers.contains("Content-Length: 5\r\n"));
        assert!(second_headers.contains("Connection: close\r\n"));
        assert_eq!(first_content, "hello");
        assert_eq!(second_content, "hello");
    }

    #[test]
    fn reads_request_body_before_the_next_request() {
        let input = b"POST /users HTTP/1.1\r\nContent-Length: 7\r\n\r\npayload\
                      GET /users HTTP/1.1\r\n\r\n";
        let mut reader = BufReader::new(&input[..]);
        let first = read_request(&mut reader).unwrap().unwrap();
        let second = read_request(&mut reader).unwrap().unwrap();

        assert!(first.ends_with("payload"));
        assert!(second.starts_with("GET /users"));
    }
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
