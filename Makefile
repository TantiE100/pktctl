.DEFAULT_GOAL := help
CARGO ?= cargo

.PHONY: help build release fmt fmt-check lint test e2e-live check clean

help: ## List the available targets
	@grep -E '^[a-z0-9-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "} {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: ## Debug build of the whole workspace
	$(CARGO) build --workspace

release: ## Optimized pktctl binary in target/release/pktctl
	$(CARGO) build --release -p pktctl

fmt: ## Format every crate
	$(CARGO) fmt --all

fmt-check: ## Fail if formatting is off
	$(CARGO) fmt --all --check

lint: ## Clippy (pedantic) with warnings as errors
	$(CARGO) clippy --workspace --all-features --all-targets -- -D warnings

test: ## Unit tests and E2E tests against the in-process fake Packet Tracer
	$(CARGO) test --workspace --all-features

e2e-live: ## E2E tests against a real Packet Tracer (needs PKTCTL_APP_ID and PKTCTL_SECRET)
	@test -n "$$PKTCTL_APP_ID" && test -n "$$PKTCTL_SECRET" || { echo "set PKTCTL_APP_ID and PKTCTL_SECRET first"; exit 1; }
	$(CARGO) test --workspace --all-features -- --ignored --test-threads=1

check: fmt-check lint test ## Everything CI runs

clean: ## Remove build artifacts
	$(CARGO) clean
