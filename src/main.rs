mod database;
mod environment_variables;
mod handle_client;
mod server;

use crate::database::{create_database_pool, set_init_database_table};
use crate::environment_variables::get_environment_variables;
use crate::handle_client::handle_client;
use crate::server::serve_connections;
use std::net::TcpListener;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment_variables = get_environment_variables();

    let pool = create_database_pool(environment_variables.databse_url)?;
    {
        let mut client = pool.get()?;
        set_init_database_table(&mut client)?;
    }

    let listener = TcpListener::bind(environment_variables.server_adress_and_port)?;
    println!(
        "Server listening on {}",
        environment_variables.server_adress_and_port
    );

    serve_connections(listener.incoming(), move |stream| {
        handle_client(stream, &pool)
    });
    Ok(())
}
