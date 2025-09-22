mod database;
mod handle_client;
mod environment_variables;

use crate::database::{create_database_client, set_init_database_table};
use std::net::{TcpListener};
use crate::environment_variables::get_environment_variables;
use crate::handle_client::handle_client;

fn main() {
    let environment_variables = get_environment_variables();

    let client = create_database_client(environment_variables.databse_url);

    if let Err(_) = set_init_database_table(client) {
        println!("Error setting database init tables");
        return;
    }

    let listener = TcpListener::bind(environment_variables.server_adress_and_port).unwrap();
    println!("Server listening on {}", environment_variables.server_adress_and_port);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                handle_client(stream, environment_variables.databse_url);
            }
            Err(e) => {
                println!("Unable to connect: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    struct MockEnvVars {
        pub databse_url: String,
        pub server_adress_and_port: String,
    }

    fn mock_get_environment_variables() -> MockEnvVars {
        MockEnvVars {
            databse_url: "postgres://localhost/test".to_string(),
            server_adress_and_port: "127.0.0.1:8080".to_string(),
        }
    }

    fn mock_create_database_client(_url: String) -> Result<(), &'static str> {
        Ok(())
    }

    fn mock_set_init_database_table(_client: Result<(), &'static str>) -> Result<(), &'static str> {
        Ok(())
    }

    #[test]
    fn test_server_initialization_success() {
        let env = mock_get_environment_variables();
        let client = mock_create_database_client(env.databse_url.clone());
        let db_init_result = mock_set_init_database_table(client);

        assert!(db_init_result.is_ok());
    }

    #[test]
    fn test_server_initialization_failure() {

        fn failing_create_database_client(_url: String) -> Result<(), &'static str> {
            Err("connection failed")
        }

        let env = mock_get_environment_variables();
        let client = failing_create_database_client(env.databse_url.clone());

        let db_init_result = mock_set_init_database_table(client);
        assert!(db_init_result.is_err());
    }

    #[test]
    fn test_listener_binding() {
        let env = mock_get_environment_variables();
        let listener = TcpListener::bind(env.server_adress_and_port.clone());
        assert!(listener.is_ok(), "Listener should bind to address");
    }
}
