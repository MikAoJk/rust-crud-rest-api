use crate::database::DatabasePool;
use postgres::Client;
use serde_derive::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;

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
            let (status_line, content) = handle_request(&request, pool);
            if let Err(error) = stream.write_all(format!("{status_line}{content}").as_bytes()) {
                eprintln!("Unable to write response: {error}");
            }
        }
        Err(error) => eprintln!("Unable to read stream: {error}"),
    }
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
    client.execute(
        "INSERT INTO users (name, email) VALUES ($1, $2)",
        &[&user.name, &user.email],
    )?;
    Ok((OK_RESPONSE.to_string(), "User created".to_string()))
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
