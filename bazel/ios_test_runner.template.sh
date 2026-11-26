#!/bin/bash
# agent-sim's test runner (ios_test_runner.bzl): the test bundle and its app, as rules_apple built them, run with
# `agent-sim xctest` on the container's phone. It reads Bazel's XML_OUTPUT_FILE, TESTBRIDGE_TEST_ONLY and
# TEST_UNDECLARED_OUTPUTS_DIR itself.
set -euo pipefail
args=(xctest "$PWD/%(test_bundle_path)s" --test-type "%(test_type)s" --timeout "%(timeout)s")
if [[ -n "%(test_host_path)s" ]]; then args+=(--app "$PWD/%(test_host_path)s"); fi
IFS=, read -ra filter <<< "%(test_filter)s"
for t in "${filter[@]}"; do args+=(--only-testing "$t"); done
export X86SIM_ROOT=${X86SIM_ROOT:-%(x86sim_root)s}
exec "%(agent_sim)s" "${args[@]}"
