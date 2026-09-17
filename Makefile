# Development tasks. The web prototype runs entirely against the offline mock, so
# everything here works on Linux with no ES-9 and no Windows toolchain.
#
#   make rundev       build the WASM bridge and serve web/ at http://localhost:8777
#   make win-install  build the desktop shell on the Windows host and install it
#
# The hardware probes stay in scripts/win-build.ps1: they take arguments and are run by
# hand, which is not what a make target is for.

PORT ?= 8777
POWERSHELL ?= powershell.exe

.DEFAULT_GOAL := help

.PHONY: help rundev serve web web-release test smoke check clean win-install

help: ## List targets
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) \
	  | sed -e 's/:.*## /\t/' \
	  | awk -F'\t' '{ printf "  \033[1m%-12s\033[0m %s\n", $$1, $$2 }'

rundev: web serve ## Build the bridge, then serve the prototype (the usual dev loop)

serve: ## Serve web/ without rebuilding
	@echo "ES-9 Mixer prototype: http://localhost:$(PORT)  (ctrl-c to stop)"
	@python3 -m http.server $(PORT) --directory web

web: ## Compile crates/es9-wasm to web/pkg (debug)
	@./build-web.sh

web-release: ## Compile crates/es9-wasm to web/pkg (release)
	@./build-web.sh release

test: ## Workspace unit and integration tests
	cargo test

smoke: ## End-to-end test through the WASM bridge, headless
	./scripts/smoke.sh

check: test smoke ## Everything that can be verified without hardware

win-install: ## Build the release shell on the Windows host and install it (WSL2 only)
	@set -e; \
	command -v $(POWERSHELL) >/dev/null 2>&1 || { \
	  echo "$(POWERSHELL) not found. This target drives the Windows toolchain from"; \
	  echo "WSL2 — the ES-9, its ASIO driver and MSVC all live on the host."; \
	  exit 1; }; \
	script="$$(wslpath -w scripts/win-install.ps1)"; \
	repo="$$(wslpath -w .)"; \
	cd /mnt/c; \
	$(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File "$$script" -Repo "$$repo"

clean: ## Drop build output, including web/pkg
	cargo clean
	rm -rf web/pkg
