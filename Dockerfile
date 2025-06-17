FROM rust:latest

ENV XDG_RUNTIME_DIR /geello

WORKDIR /geello

COPY ./assets ./assets

RUN apt update && apt install -y libvulkan1 && cargo install -F server geello

CMD ["geello"]
