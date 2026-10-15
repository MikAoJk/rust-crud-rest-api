# performance-test

### Prerequisites
1. [rust](../README.md#Rust)
2. [docker](../README.md#Docker)
3. [docker compose](../README.md#Docker-Compose)
4. [curl](../README.md#Curl)
5. k6
   Make sure you have k6 installed using this command:
```bash script
k6 --version
```
or install it: https://grafana.com/docs/k6/latest/set-up/install-k6/


## Running k6 performance-test
1. [Build code](../README.md#Build-code)
2. [Create docker image of the app](../README.md#Create-docker-image-of-the-app)
3. [Run the Postgres container and app](../README.md#Run-the-Postgres-container-and-app)
4. Run the performance test 
```bash script
cd performance-test
k6 run -e BASE_URL=http://localhost:8080 --vus 1 --duration 60s script.js
```
