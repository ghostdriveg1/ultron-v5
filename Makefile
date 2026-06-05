# Phase 5 — CI/CD and Dev Config

# Makefile for Olympus V5

HF_TOKEN ?= $(shell echo $$HF_TOKEN)
ORG       := ultron-v5

.PHONY: help dev build check deploy-gateway deploy-memory push-all

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

check: ## cargo check (compile check without building)
	cargo check --workspace

build: ## cargo build release
	cargo build --release --workspace

test: ## run all tests
	cargo test --workspace

dev: ## run gateway locally (needs .env file with secrets)
	RUST_LOG=info cargo run -p gateway

push-gateway: ## Push gateway source to HF Space 1
	@echo "Pushing gateway to ultron-v5/olympus-gateway..."
	cd /tmp/olympus-gateway-push && \
	git init && \
	git remote add space https://huggingface.co/spaces/$(ORG)/olympus-gateway && \
	cp -r $(CURDIR)/gateway $(CURDIR)/dashboard $(CURDIR)/Cargo.toml $(CURDIR)/Dockerfile . && \
	git add -A && \
	git commit -m "Deploy gateway v5.0.0" && \
	git push --force space main

push-memory: ## Push memory shard config to all 4 HF Memory Spaces
	@for space in olympus-memory-l1 olympus-memory-l3 olympus-memory-l4 olympus-memory-rnd; do \
		echo "Pushing to $(ORG)/$$space..."; \
		tmpdir=$$(mktemp -d); \
		cp -r $(CURDIR)/spaces/memory-base/* $$tmpdir/; \
		cd $$tmpdir && git init && \
		git remote add space https://huggingface.co/spaces/$(ORG)/$$space && \
		git add -A && \
		git commit -m "Deploy OMNIMEM shard" && \
		git push --force space main; \
		cd $(CURDIR); \
	done

push-all: push-memory push-gateway ## Deploy everything to HF Spaces

clean: ## Remove build artifacts
	cargo clean
