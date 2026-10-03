.PHONY: build run window test check browser-test fixtures

build:
	npm ci --prefix apps/desktop
	npm run --prefix apps/desktop build
	cargo build --workspace --jobs 2

run:
	cargo run -- serve

window:
	/usr/bin/python3 scripts/window.py

test:
	cargo test --workspace --jobs 2
	npm run --prefix apps/desktop test
	/usr/bin/python3 -m unittest discover -s integrations/tests -p 'test_*.py'
	node --test integrations/tests/gnome-extension.test.mjs

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --jobs 2 -- -D warnings
	npm run --prefix apps/desktop build

fixtures:
	/usr/bin/python3 tests/create_fixtures.py

browser-test:
	npm ci --prefix tests
	npm run --prefix tests browser
	npm run --prefix tests browser:playback
