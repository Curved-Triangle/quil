SHELL := /bin/sh

.PHONY: build run models test check tools install

build:
	scripts/cargo.sh build --release --workspace

run: build
	target/release/quil dictate

models: build
	target/release/quil-model-fetch download moonshine-tiny

test:
	scripts/cargo.sh test --workspace --locked

check:
	scripts/cargo.sh fmt --all -- --check
	scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
	scripts/cargo.sh test --workspace --locked
	.tools/cargo/bin/cargo-audit audit --deny warnings
	.tools/cargo/bin/cargo-deny check
	scripts/check-runtime-network-deps.sh

tools:
	cargo install --root .tools/cargo cargo-audit --version 0.22.2 --locked
	cargo install --root .tools/cargo cargo-deny --version 0.20.2 --locked

install: build
	scripts/install.sh
