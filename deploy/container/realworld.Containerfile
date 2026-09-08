FROM docker.io/library/archlinux:latest

RUN useradd -u 10001 -U -d /home/aequora -m -s /bin/sh aequora && \
    mkdir -p /etc/aequora /tmp && \
    chown -R aequora:aequora /etc/aequora /tmp && \
    chmod 1777 /tmp

COPY --chown=10001:10001 target/release/examples/realworld_server /usr/local/bin/aequora-server

USER 10001:10001
WORKDIR /home/aequora

ENV PORT=8443 \
    HOST=0.0.0.0 \
    RUST_LOG=info

EXPOSE 8443

ENTRYPOINT ["/usr/local/bin/aequora-server"]
