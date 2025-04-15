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

    //start server and print port
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

