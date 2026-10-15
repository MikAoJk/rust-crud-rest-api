# performance-test

### Prerequisites
1. [rust](../README.md#Rust)
2. [docker](../README.md#Docker)
3. [docker compose](../README.md#Docker-Compose)
4. [curl](../README.md#Curl)

#### k6
Make sure you have k6 installed using this command:
```bash script
k6 --version
```
or install it: https://grafana.com/docs/k6/latest/set-up/install-k6/


## Running k6 performance-test
1. [Build code](../README.md#Build-code)
2. [Create docker image of the app](../README.md#Create-docker-image-of-the-app)
3. [Run the Postgres container and app](../README.md#Run-the-Postgres-container-and-app)
4. Curl request to create a new user
```bash script
curl --location --request POST 'http://localhost:8080/users' \
--header 'Content-Type: application/json' \
--data-raw '{"name": "aaa","email": "aaa@mail"}'
``` 
and then you can run the performance test 
```bash script
cd performance-test
k6 run script.js
```
Example output:
TOTAL RESULTS

    checks_total.......: 300     9.868267/s
    checks_succeeded...: 100.00% 300 out of 300
    checks_failed......: 0.00%   0 out of 300

    ✓ status is 200

    HTTP
    http_req_duration..............: avg=10.75ms min=3.03ms med=9.48ms max=48.93ms p(90)=15.3ms p(95)=21.7ms
      { expected_response:true }...: avg=10.75ms min=3.03ms med=9.48ms max=48.93ms p(90)=15.3ms p(95)=21.7ms
    http_req_failed................: 0.00% 0 out of 300
    http_reqs......................: 300   9.868267/s

    EXECUTION
    iteration_duration.............: avg=1.01s   min=1s     med=1.01s  max=1.05s   p(90)=1.01s  p(95)=1.02s 
    iterations.....................: 300   9.868267/s
    vus............................: 10    min=10       max=10
    vus_max........................: 10    min=10       max=10

    NETWORK
    data_received..................: 27 kB 898 B/s
    data_sent......................: 23 kB 760 B/s
