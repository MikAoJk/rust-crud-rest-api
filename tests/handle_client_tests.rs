use rust_crud_rest_api::handle_client::{get_id, get_user_request_body, handle_post_request, handle_get_all_request, handle_put_request, handle_delete_request};

#[test]
fn test_get_id() {
    let req = "GET /users/42 HTTP/1.1\r\nHost: localhost\r\n\r\n";
    assert_eq!(get_id(req), "42");
    let req = "GET /users/abc HTTP/1.1\r\n";
    assert_eq!(get_id(req), "abc");
    let req = "GET /users/ HTTP/1.1\r\n";
    assert_eq!(get_id(req), "");
}

#[test]
fn test_get_user_request_body_valid() {
    let req = "POST /users HTTP/1.1\r\nHost: localhost\r\n\r\n{\"name\":\"Alice\",\"email\":\"alice@example.com\"}";
    let user = get_user_request_body(req).unwrap();
    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.id, None);
}

#[test]
fn test_get_user_request_body_invalid() {
    let req = "POST /users HTTP/1.1\r\nHost: localhost\r\n\r\nnot a json";
    assert!(get_user_request_body(req).is_err());
}

use std::env;
fn get_test_db_url() -> String {
    env::var("TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:password@localhost/testdb".to_string())
}

#[test]
#[ignore]
fn test_handle_post_and_get_all_request() {
    let db_url = get_test_db_url();
    
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
