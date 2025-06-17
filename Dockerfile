FROM rust:latest

WORKDIR /geello

COPY ./assets ./assets

RUN cargo install -F server geello

CMD ["geello"]
