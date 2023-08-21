use postgres::{Client, NoTls};
use postgres::Error as PostgresError;

pub fn set_init_database_table(postgresclient: Client) -> Result<(), PostgresError> {
    let mut client = postgresclient;
    client.batch_execute(
        "
        CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            name VARCHAR NOT NULL,
            email VARCHAR NOT NULL
        )
    "
    )?;
    Ok(())
}

pub fn create_database_client(databse_url: &str) -> Client {
    return Client::connect(databse_url.as_ref(), NoTls).unwrap();
}
