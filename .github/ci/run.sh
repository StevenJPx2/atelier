#!/bin/bash
# GitHub-hosted Linux only. Never use this runner on a developer machine.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == Linux \
  && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]]

export MODE=${1:?mode}
export GATE=${2:-}
export ROOT=${3:-.}
export BASE=${4:-}
reports="$RUNNER_TEMP/atelier-reports"
mkdir -p "$reports" "$RUNNER_TEMP/atelier"

support=${CI_SUPPORT:?pinned CI support directory}
docker build --tag atelier-gates --file "$support/Dockerfile" "$support" >&2
name="atelier-gates-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$RANDOM"
trap 'docker rm --force "$name" >/dev/null 2>&1 || true' EXIT

# Leave the runner one CPU and 2 GiB; any hosted runner size fits.
cpus=$(( $(nproc) > 1 ? $(nproc) - 1 : 1 ))
total=$(awk '/^MemTotal:/ { print int($2 / 1024) }' /proc/meminfo)
memory="$(( total > 4096 ? total - 2048 : total / 2 ))m"

# No runner home, credentials, command files or container-engine socket.
# Omitting --pid keeps Docker's private PID namespace; bridge is not host net.
# The process limit counts threads: three mutant jobs run rustc and nextest.
docker create --name "$name" --init --user 10001:10001 \
  --cap-drop ALL --security-opt no-new-privileges \
  --pids-limit 4096 --memory "$memory" --memory-swap "$memory" --cpus "$cpus" \
  --network bridge --ipc private --stop-timeout 10 \
  --mount "type=bind,source=${SOURCE:?checkout},target=/source,readonly" \
  --mount "type=bind,source=$RUNNER_TEMP/atelier,target=/binary,readonly" \
  --env MODE --env GATE --env ROOT --env BASE atelier-gates >/dev/null
docker inspect "$name" > "$reports/container.json"

status=0
timeout --signal=TERM --kill-after=30s 75m \
  docker start --attach "$name" || status=$?
# Evidence collection cannot turn a failed, timed-out or killed run green.
docker cp "$name:/work/ci-reports/." "$reports/" >&2 || true
docker cp "$name:/work/mutants.out" "$reports/mutants.out" >&2 || true
# Say which evidence a gate rests on: the diff's mutants or the canary's.
for note in base.txt summary.md; do
  if [[ -f "$reports/$note" && -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    cat "$reports/$note" >> "$GITHUB_STEP_SUMMARY"
  fi
done
exit "$status"
