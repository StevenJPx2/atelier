#!/bin/bash
set -euo pipefail

# The host checkout is read-only and has no persisted checkout credential.
# All source execution, including the planner's process runner, stays here.
cp -R /source/. /work/
git config --global --add safe.directory /work
# Reports must come from this run, never from files the branch committed.
rm -rf /work/ci-reports /work/mutants.out /work/mutants.out.old
mkdir -p /work/ci-reports

if [[ "$MODE" == selftest ]]; then
  cd /opt/ci
  python3 -m unittest -v test_reports
  pipx run ruff==0.17.4 --version
  exit
fi

diff=()
# A push's `before` is all zeros on a branch's first push and unreachable
# after a force push. Then every gate is planned, mutants use the canary,
# and the job summary says so; it never passes as an empty diff.
if [[ "${BASE:-}" =~ ^0+$ ]] || { [[ -n "${BASE:-}" ]] \
  && ! git -C /work cat-file -e "$BASE^{commit}" 2>/dev/null; }; then
  echo "base $BASE is unavailable (first or forced push): every gate runs and mutants use the canary" \
    > /work/ci-reports/base.txt
  cat /work/ci-reports/base.txt >&2
  BASE=
fi

if [[ -n "${BASE:-}" ]]; then
  diff=(--diff "$BASE")
fi

if [[ "$MODE" == plan ]]; then
  cargo build --locked -p atelier-cli >&2
  cp target/debug/atelier ci-reports/atelier
  target/debug/atelier gates --json "${diff[@]}"
  exit
fi

if [[ "$MODE" == plan-release ]]; then
  /binary/atelier gates --json "${diff[@]}"
  exit
fi

# Do not turn a missing tool, killed container or missing report into a pass.
set +e
/binary/atelier check --setup --gate "$GATE" --root "$ROOT" "${diff[@]}" 2>&1 \
  | tee ci-reports/gate.log
codes=("${PIPESTATUS[@]}")
set -e
status=${codes[0]}
if [[ ${codes[1]} -ne 0 && $status -eq 0 ]]; then
  status=1
fi
python3 /opt/ci/reports.py "$GATE" "/work/$ROOT" --summary ci-reports/summary.md || status=1
exit "$status"
