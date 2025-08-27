FROM rust:1.89.0

WORKDIR /app
RUN apt update && apt install lld clang -y 
COPY . .
RUN cargo build --release 
ENTRYPOINT ["/app/target/release/zero2prod"]