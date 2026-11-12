set windows-shell := ["powershell"]

install:
    cargo install --path .

lint:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets
