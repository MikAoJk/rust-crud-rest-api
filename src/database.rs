use postgres::Error as PostgresError;
use postgres::{Client, NoTls};
use r2d2_postgres::PostgresConnectionManager;
use std::time::Duration;

pub const MAX_CONNECTIONS: u32 = 64;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
pub type DatabasePool = r2d2::Pool<PostgresConnectionManager<NoTls>>;

pub fn set_init_database_table(client: &mut Client) -> Result<(), PostgresError> {
    client.batch_execute(
        "
        CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            name VARCHAR NOT NULL,
            email VARCHAR NOT NULL
        )
    ",
    )?;
    Ok(())
}

pub fn create_database_pool(
    database_url: &str,
) -> Result<DatabasePool, Box<dyn std::error::Error>> {
    let manager = PostgresConnectionManager::new(database_url.parse()?, NoTls);
    Ok(r2d2::Pool::builder()
        .max_size(MAX_CONNECTIONS)
        .connection_timeout(CONNECTION_TIMEOUT)
        .build(manager)?)
}
