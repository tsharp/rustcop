.PHONY: build check fix publish

build:
	cargo build

check:
	cargo run -p rustcop -- check

fix:
	cargo run -p rustcop -- fix

publish:
	cargo publish -p rustcop
	cargo publish -p cargo-rustcop
