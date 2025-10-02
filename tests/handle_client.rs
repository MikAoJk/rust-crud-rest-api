use std::net::{TcpListener, TcpStream};
use std::thread;
use std::io::{Read, Write};
use std::sync::Once;

use rust_crud_rest_api::handle_client::handle_client;

static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
    });
}

fn mock_database_url() -> String {
    "postgres://localhost/test".to_string()
}

// Helper to simulate a client sending a request
fn send_request(request: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let db_url = mock_database_url();
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            handle_client(stream, &db_url);
        }
    });


    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn test_post_user() {
    setup();
    let request = "POST /users HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"name\":\"Test\",\"email\":\"test@example.com\"}";
    let response = send_request(request);
    assert!(response.contains("User created"));
}

#[test]
fn test_get_user_not_found() {
    setup();
    let request = "GET /users/999 HTTP/1.1\r\n\r\n";
    let response = send_request(request);
    assert!(response.contains("User not found") || response.contains("404"));
}

#[test]
fn test_post_user() {
    setup();
    let request = "POST /users HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"name\":\"Test\",\"email\":\"test@example.com\"}";
    let response = send_request(request);
    assert!(response.contains("User created"));
}

#[test]
fn test_get_user_not_found() {
    setup();
    let request = "GET /users/999 HTTP/1.1\r\n\r\n";
    let response = send_request(request);
    assert!(response.contains("User not found") || response.contains("404"));

    let req = "POST /users HTTP/1.1\r\n\r\n{\"name\":\"Bob\",\"email\":\"bob@example.com\"}";
    let (status, body) = handle_post_request(req, &db_url);
    assert!(status.contains("200 OK"));
    assert_eq!(body, "User created");

    let (status, body) = handle_get_all_request("GET /users HTTP/1.1\r\n\r\n", &db_url);
    assert!(status.contains("200 OK"));
    assert!(body.contains("bob@example.com"));
}

#[test]
#[ignore]
fn test_handle_put_and_delete_request() {
    let db_url = get_test_db_url();
    
    let req = "POST /users HTTP/1.1\r\n\r\n{\"name\":\"Charlie\",\"email\":\"charlie@example.com\"}";
    let _ = handle_post_request(req, &db_url);

    
    let req = "PUT /users/1 HTTP/1.1\r\n\r\n{\"name\":\"Charles\",\"email\":\"charles@example.com\"}";
    let (status, body) = handle_put_request(req, &db_url);
    assert!(status.contains("200 OK"));
    assert_eq!(body, "User updated");

    
    let req = "DELETE /users/1 HTTP/1.1\r\n\r\n";
    let (status, body) = handle_delete_request(req, &db_url);
    assert!(status.contains("200 OK"));
    assert_eq!(body, "User deleted");
}
