.PHONY: doctor dev dev-check test release-preflight release-signing-preflight benchmark-readiness benchmark-readiness-full smoke prompt-smoke llm-connectivity-smoke llm-core-smoke llm-practical-smoke llm-decision-smoke llm-structured-smoke llm-grounded-smoke llm-reliability-smoke code-edit-smoke code-edit-contract-smoke security-smoke worker-harness-contract-smoke cloud-contract-smoke cloud-provider-job-smoke cloud-catalog-smoke local-runtime-discovery-smoke live-cloud-smoke provider-error-contract-smoke validation-contract-smoke create-target-handoff-smoke local-cloud-connectivity-smoke local-cloud-compare-smoke local-cloud-job-smoke local-cloud-basics-smoke local-cloud-core-smoke local-cloud-practical-smoke local-cloud-decision-smoke local-cloud-structured-smoke local-cloud-grounded-smoke local-cloud-reliability-smoke smoke-docker job-smoke report-smoke first-run-smoke worker-smoke hf-search-smoke hf-download-smoke hf-download-job-smoke hf-download-start-job-smoke hf-server-job-smoke hf-server-start-job-smoke hf-local-smoke hf-local-cloud-smoke hf-local-cloud-basics-smoke validate-schemas app-build rust-test worker-test worker-help verify-dmg verify-distribution-dmg install-smoke-dmg package-dmg package-release-dmg

PYTHON ?= $(if $(wildcard workers/.venv/bin/python),workers/.venv/bin/python,$(or $(BENCHFORGE_PYTHON),python3))

doctor:
	./scripts/doctor.sh

dev:
	cd app-scaffold && npm run tauri:dev

dev-check:
	python3 scripts/check-tauri-dev-launch.py

test: validate-schemas app-build rust-test worker-test worker-help

release-preflight:
	BENCHFORGE_PYTHON="$(PYTHON)" ./scripts/release-preflight.sh

release-signing-preflight:
	./scripts/release-signing-preflight-macos.sh

benchmark-readiness:
	./scripts/benchmark-readiness.sh quick

benchmark-readiness-full:
	./scripts/benchmark-readiness.sh full

smoke:
	./scripts/run-smoke-local.sh

prompt-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-prompt-smoke

llm-connectivity-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-connectivity-smoke

llm-core-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-core-smoke

llm-practical-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-practical-smoke

llm-decision-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-decision-smoke

llm-structured-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-structured-smoke

llm-grounded-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-grounded-smoke

llm-reliability-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-llm-reliability-smoke

code-edit-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-code-edit-smoke

code-edit-contract-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-code-edit-contract-smoke

security-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-security-smoke

worker-harness-contract-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-worker-harness-contract-smoke

cloud-contract-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-cloud-contract-smoke

cloud-provider-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-cloud-provider-job-smoke

cloud-catalog-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-cloud-catalog-smoke

local-runtime-discovery-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-runtime-discovery-smoke

live-cloud-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-live-cloud-smoke

provider-error-contract-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-provider-error-contract-smoke

validation-contract-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-validation-contract-smoke

create-target-handoff-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-create-target-handoff-smoke

local-cloud-connectivity-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-connectivity-smoke

local-cloud-compare-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-compare-smoke

local-cloud-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-job-smoke

local-cloud-basics-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-basics-smoke

local-cloud-core-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-core-smoke

local-cloud-practical-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-practical-smoke

local-cloud-decision-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-decision-smoke

local-cloud-structured-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-structured-smoke

local-cloud-grounded-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-grounded-smoke

local-cloud-reliability-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-local-cloud-reliability-smoke

job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-job-smoke

report-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-report-smoke

first-run-smoke:
	@tmp="$$(mktemp -d "$${TMPDIR:-/tmp}/benchforge-first-run.XXXXXX")"; \
	trap 'rm -rf "$$tmp"' EXIT; \
	cd app-scaffold/src-tauri && BENCHFORGE_DATA_DIR="$$tmp" cargo run -- --benchforge-first-run-smoke

smoke-docker:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-smoke --docker

worker-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-worker-mock

hf-search-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-search-smoke

hf-download-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-download-smoke

hf-download-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-download-job-smoke

hf-download-start-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-download-start-job-smoke

hf-server-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-server-job-smoke

hf-server-start-job-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-server-start-job-smoke

hf-local-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-local-smoke

hf-local-cloud-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-local-cloud-smoke

hf-local-cloud-basics-smoke:
	cd app-scaffold/src-tauri && cargo run -- --benchforge-hf-local-cloud-basics-smoke

validate-schemas:
	$(PYTHON) scripts/validate-schemas.py

app-build:
	cd app-scaffold && npm run build:web

rust-test:
	cd app-scaffold/src-tauri && cargo test

worker-test:
	$(PYTHON) -m unittest discover -s workers/tests

worker-help:
	$(PYTHON) -m benchforge_worker.cli --help >/dev/null

verify-dmg:
	./scripts/verify-dmg-macos.sh

verify-distribution-dmg:
	./scripts/verify-macos-distribution.sh

install-smoke-dmg:
	./scripts/verify-dmg-install-smoke-macos.sh

package-dmg:
	$(MAKE) release-preflight
	./scripts/package-dmg-macos.sh

package-release-dmg:
	$(MAKE) release-preflight
	BENCHFORGE_RELEASE_DISTRIBUTION=1 ./scripts/package-dmg-macos.sh
