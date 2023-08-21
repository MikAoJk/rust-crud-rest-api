use std::env;
use serde_derive::{Deserialize, Serialize};

pub fn get_environment_variables() -> EnvironmentVariables {
    let databse_url: &str = string_to_static_str(env::var("DATABASE_URL")
        .unwrap_or("environment variable DATABASE_URL is missing".to_string()));
    let server_adress_and_port: &str = "0.0.0.0:8080";


    return EnvironmentVariables {
        databse_url,
        server_adress_and_port
    };
}

fn string_to_static_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[derive(Serialize, Deserialize)]
pub struct EnvironmentVariables {

    pub(crate) databse_url: &'static str,
    pub(crate) server_adress_and_port: &'static str
}
