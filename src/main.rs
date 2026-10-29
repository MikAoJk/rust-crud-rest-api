mod database;
mod environment_variables;
mod handle_client;
mod metrics;
mod server;

use crate::database::{create_database_pool, set_init_database_table};
use crate::environment_variables::get_environment_variables;
use crate::handle_client::handle_client;
use crate::server::serve_connections;
use std::net::TcpListener;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment_variables = get_environment_variables();

    let (pool, listener) =
        initialize_server(environment_variables.server_adress_and_port, || {
            let pool = create_database_pool(environment_variables.databse_url)?;
            {
                let mut client = pool.get()?;
                set_init_database_table(&mut client)?;
            }
            Ok(pool)
        })?;
    println!(
        "Server listening on {}",
        environment_variables.server_adress_and_port
    );

    serve_connections(listener.incoming(), move |stream| {
        handle_client(stream, &pool)
    });
    Ok(())
}

fn initialize_server<T>(
    address: &str,
    initialize_database: impl FnOnce() -> Result<T, Box<dyn std::error::Error>>,
) -> Result<(T, TcpListener), Box<dyn std::error::Error>> {
    let database = initialize_database()?;
    let listener = TcpListener::bind(address)?;
    Ok((database, listener))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::net::TcpStream;

    #[test]
    fn server_initialization_returns_database_and_bound_listener() {
        let (database, listener) =
            initialize_server("127.0.0.1:0", || Ok(String::from("initialized database")))
                .unwrap();

        assert_eq!(database, "initialized database");
        let address = listener.local_addr().unwrap();
        assert!(address.ip().is_loopback());
        assert_ne!(address.port(), 0);
        let connection = TcpStream::connect(address).unwrap();
        let (_, peer_address) = listener.accept().unwrap();
        assert_eq!(peer_address, connection.local_addr().unwrap());
    }

    #[test]
    fn server_initialization_propagates_database_error_before_binding() {
        let result = initialize_server::<()>("invalid address", || {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "database unavailable").into())
        });

        let error = result.unwrap_err();
        let error = error.downcast_ref::<io::Error>().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "database unavailable");
    }

    #[test]
    fn server_initialization_propagates_listener_binding_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut database_initialized = false;

        let result = initialize_server(&address, || {
            database_initialized = true;
            Ok(())
        });

        assert!(database_initialized);
        let error = result.unwrap_err();
        let error = error.downcast_ref::<io::Error>().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    }
}
