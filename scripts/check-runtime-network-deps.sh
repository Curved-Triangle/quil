#!/bin/sh
set -eu

# Build-time downloaders are allowed in ort-sys. The shipped runtime's normal
# dependency graph must not contain an HTTP/TLS client implementation.
if cargo tree -p quil --edges normal --prefix none |
    grep -E '(^| )(reqwest|ureq|hyper|native-tls|openssl|curl|isahc|reqwest) ' >/dev/null
then
    echo "HTTP/TLS client found in Quil's runtime dependency graph" >&2
    cargo tree -p quil --edges normal --prefix none |
        grep -E 'reqwest|ureq|hyper|native-tls|openssl|curl|isahc' >&2 || true
    exit 1
fi
