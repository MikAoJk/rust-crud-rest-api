# rust-crud-rest-api

### Prerequisites
Make sure you have the rust installed using this command:
#### Rust
```bash script
rustc --version
```

#### Cargo
Make sure you have cargo installed using this command:
```bash script
cargo --version
```

#### Docker
Make sure you have cargo installed using this command:
```bash script
docker --version
```

#### Running the application locally
##### 🏗️ Build the Rust app image
Creating a docker image should be as simple as
``` bash
docker build -t rustapp .
```

##### 🐘 Run the Postgres container
```bash script
docker-compose up -d db
```

##### 🏗️ Build the Rust app image
Creating a docker image should be as simple as
``` bash
docker build -t rustapp .
```

##### 🐘 Run the Postgres container
```bash script
docker-compose up -d db
```

##### 🏗️ Build the Rust app image
```bash script
docker compose build
```

##### 👟 Run the Rust Container
```bash script
docker compose up rustapp
```

##### 🧪 Test the applications endpoints
For testing the endpoints
You need a tool to send a request and to inspect the repsonse
A tool you can use is Postman: https://www.postman.com/downloads/

Request to get the users:
```bash script
curl --location --request GET 'http://localhost:8080/users'
```
Example of a response:
`{[]}`

Create a new user
```bash script
curl --location --request POST 'http://localhost:8080/users' \
--header 'Content-Type: application/json' \
--data-raw '{"name": "aaa","email": "aaa@mail"}'
```

### Build code
Build the code without running it
```bash script
cargo build
```

### Run code
Build and run the code, showing the output
```bash script
cargo run
```

### Test code
Build the code and run all the tests
```bash script
cargo test
```

## Contact
This project is maintained by [MikAoJk](CODEOWNERS)