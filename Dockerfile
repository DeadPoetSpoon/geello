FROM rust:latest

WORKDIR /geello

COPY ./assets ./assets

RUN apt update && apt install -y libvulkan1 && cargo install -F server geello

CMD ["geello"]
