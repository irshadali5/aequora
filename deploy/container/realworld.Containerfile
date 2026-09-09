FROM docker.io/library/archlinux@sha256:818793c894d94534c22f2149154a39ebaee57e4e67321023b0866a1d5722036c

RUN useradd -u 10001 -U -d /home/aequora -m -s /bin/sh aequora && \
    mkdir -p /etc/aequora /tmp && \
    chown -R aequora:aequora /etc/aequora /tmp && \
    chmod 1777 /tmp

COPY --chown=10001:10001 target/release/examples/realworld_server /usr/local/bin/aequora-server

USER 10001:10001
WORKDIR /home/aequora

ENV PORT=8443 \
    HOST=0.0.0.0 \
    AEQUORA_STRESS_ALLOW_CONTAINER_BIND=1 \
    RUST_LOG=info

EXPOSE 8443

ENTRYPOINT ["/usr/local/bin/aequora-server"]
