# Baked in rather than bind-mounted: against a remote Docker context a
# ./Caddyfile bind mount would resolve on the remote host.
FROM caddy:2-alpine

COPY Caddyfile /etc/caddy/Caddyfile
