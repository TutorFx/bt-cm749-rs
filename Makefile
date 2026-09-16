.PHONY: build test test-all lint static e2e install uninstall clean

BIN := target/release/bt-cm749
STATIC := dist/bt-cm749-x86_64-linux-musl
ALPINE_BUILD := docker run --rm -v "$(CURDIR)":/src:ro -v "$(CURDIR)/dist":/dist \
	-v bt-cm749-cargo:/usr/local/cargo/registry -v bt-cm749-target:/target \
	-w /src -e CARGO_TARGET_DIR=/target rust:1-alpine sh -c

build:
	cargo build --release --locked

# Unit + sandboxed integration tests (no root, no network)
test:
	cargo test --locked

# Also downloads kernel tarballs, compiles btusb.ko against the running kernel's
# headers and compares with the shell implementation in ../bt-cm749-fix
test-all:
	cargo test --locked -- --include-ignored

lint:
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings

# Fully static musl binary, built in an Alpine container
static:
	mkdir -p dist
	$(ALPINE_BUILD) 'apk add -q musl-dev make perl && \
		cargo build -q --release --locked --target x86_64-unknown-linux-musl && \
		cp /target/x86_64-unknown-linux-musl/release/bt-cm749 /$(STATIC) && \
		chown $(shell id -u):$(shell id -g) /$(STATIC)'

# Real DKMS install/uninstall cycle in disposable containers
e2e: static
	for img in archlinux:latest fedora:41; do \
		docker run --rm -v "$(CURDIR)/dist":/bt:ro -v "$(CURDIR)/ci":/ci:ro $$img \
			/ci/dkms-e2e.sh /bt/bt-cm749-x86_64-linux-musl || exit 1; \
	done

install: build
	sudo $(BIN) install

uninstall: build
	sudo $(BIN) uninstall

clean:
	cargo clean
	rm -rf dist /tmp/bt_test_sandbox_* /tmp/bt_kernel_test_* /tmp/bt_real_build_*
