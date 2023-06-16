use postgres::{Client, NoTls};
use postgres::Error as PostgresError;

//DATABASE URL
//const DB_URL: &str = env!("DATABASE_URL");
pub const DB_URL: &str = "postgres://postgres:postgres@db:5432/postgres";

//db setup
pub fn set_database() -> Result<(), PostgresError> {
    let mut client = Client::connect(DB_URL, NoTls)?;
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