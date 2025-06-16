FROM rust:latest

RUN cargo install -F server geello

CMD ["geello"]
