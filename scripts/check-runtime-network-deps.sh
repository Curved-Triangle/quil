#!/bin/sh
set -eu

# Build-time downloaders are allowed in ort-sys. The shipped runtime's normal
# dependency graph must not contain an HTTP/TLS client implementation or any
# general-purpose HTTP stack. `quil-model-fetch` is the only crate permitted to
# depend on a network client, and it is not a runtime crate.
PATTERN='(^| )(reqwest|ureq|hyper(-[a-z]+)?|http|httparse|http-body(-util)?|native-tls|tokio-(native|rust)ls|rustls(-[a-z]+)?|tls-api(-[a-z]+)?|openssl|curl|isahc|minreq|attohttpc|surf|fasthttp|awc|quinn) '

# Fail closed: a `cargo tree` that cannot run proves nothing about the graph.
# Treating its failure as "no matches" would report a clean runtime after an
# incomplete or errored enumeration.
for package in quil quil-audio quil-core quil-inference; do
    if ! tree=$(cargo tree --locked -p "$package" --edges normal --prefix none); then
        echo "cargo tree failed for $package; refusing to report a clean dependency graph" >&2
        exit 2
    fi
    if printf '%s\n' "$tree" | grep -E "$PATTERN" >/dev/null; then
        echo "HTTP/TLS crate found in the runtime dependency graph of $package:" >&2
        printf '%s\n' "$tree" | grep -E "$PATTERN" >&2 || true
        exit 1
    fi
done
echo "No HTTP/TLS client crates in the runtime dependency graph."
