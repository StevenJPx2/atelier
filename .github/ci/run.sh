#!/bin/bash
# GitHub-hosted Linux only. Never use this runner on a developer machine.
set -euo pipefail
if [[ "${GITHUB_ACTIONS:-}" != true || "${RUNNER_OS:-}" != Linux \
  || "${RUNNER_ENVIRONMENT:-}" != github-hosted ]]; then
  echo "run.sh runs only on a GitHub-hosted Linux runner; run checks in CI" >&2
  exit 2
fi

export MODE=${1:?mode}
export GATE=${2:-}
export ROOT=${3:-.}
export BASE=${4:-}
export PROJECT=${GITHUB_REPOSITORY:-}
reports="$RUNNER_TEMP/atelier-reports"
mkdir -p "$reports" "$RUNNER_TEMP/atelier"

# Each job stays inside its 90 minutes: plan is selftest + plan, a gate is
# gate + validate.
case "$MODE" in
  selftest | plan-release) limit=10m ;;
  plan) limit=60m ;;
  gate) limit=75m ;;
  *) echo "unknown mode $MODE" >&2; exit 2 ;;
esac

support=${CI_SUPPORT:?pinned CI support directory}
docker build --tag atelier-gates --file "$support/Dockerfile" "$support" >&2
names=()
trap 'docker rm --force "${names[@]}" >/dev/null 2>&1 || true' EXIT

# Leave the runner one CPU and 2 GiB; any hosted runner size fits.
cpus=$(( $(nproc) > 1 ? $(nproc) - 1 : 1 ))
total=$(awk '/^MemTotal:/ { print int($2 / 1024) }' /proc/meminfo)
memory="$(( total > 4096 ? total - 2048 : total / 2 ))m"

# create NAME NETWORK MOUNT...: no runner home, credentials, command files or
# container-engine socket. Omitting --pid keeps Docker's private PID
# namespace. The process limit counts threads: three mutant jobs run rustc
# and nextest.
create() {
  local name=$1 network=$2
  shift 2
  local mounts=()
  for mount in "$@"; do
    mounts+=(--mount "$mount")
  done
  docker create --name "$name" --init --user 10001:10001 \
    --cap-drop ALL --security-opt no-new-privileges \
    --pids-limit 4096 --memory "$memory" --memory-swap "$memory" --cpus "$cpus" \
    --network "$network" --ipc private --stop-timeout 10 \
    "${mounts[@]}" \
    --env MODE --env GATE --env ROOT --env BASE --env PROJECT atelier-gates >/dev/null
  names+=("$name")
  contained "$name"
}

# Refuse to start a container whose recorded settings are not the contained
# ones; the inspection is kept as evidence.
contained() {
  docker inspect "$1" > "$reports/$1.json"
  jq -e '.[0] as $c | $c.Config.User == "10001:10001"
    and $c.HostConfig.CapDrop == ["ALL"]
    and ($c.HostConfig.SecurityOpt | any(startswith("no-new-privileges")))
    and $c.HostConfig.Privileged == false
    and ($c.HostConfig.PidMode == "" or $c.HostConfig.PidMode == "private")
    and $c.HostConfig.IpcMode == "private"
    and ($c.HostConfig.NetworkMode == "bridge" or $c.HostConfig.NetworkMode == "none")
    and $c.HostConfig.PidsLimit == 4096 and $c.HostConfig.Memory > 0
    and ($c.HostConfig.Binds // [] | length == 0)
    and ($c.HostConfig.Mounts | all(.ReadOnly == true))
    and ($c.HostConfig.Mounts | all(.Source != "/var/run/docker.sock"))' \
    "$reports/$1.json" >/dev/null || { echo "container $1 is not contained" >&2; exit 1; }
}

# start NAME: run it within the mode's limit; its exit status.
start() {
  timeout --signal=TERM --kill-after=30s "$limit" docker start --attach "$1"
}

id="$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$RANDOM"
create "atelier-$MODE-$id" bridge \
  "type=bind,source=${SOURCE:?checkout},target=/source,readonly" \
  "type=bind,source=$RUNNER_TEMP/atelier,target=/binary,readonly"

status=0
start "atelier-$MODE-$id" || status=$?

# Evidence collection cannot turn a failed, timed-out or killed run green.
# docker cp keeps links, so only plain files and folders cross to the host.
docker cp "atelier-$MODE-$id:/reports/." "$reports/" >&2 || true
find "$reports" -mindepth 1 ! -type f ! -type d -delete

if [[ "$MODE" == gate ]]; then
  # A fresh, offline container judges the copied reports: the gate cannot
  # change the validator or its interpreter.
  MODE=validate
  limit=10m
  create "atelier-validate-$id" none "type=bind,source=$reports,target=/reports,readonly"
  verdict=$(start "atelier-validate-$id") || status=1
  echo "$verdict"
  if [[ -n "$verdict" && -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '%s\n\n' "$verdict" >> "$GITHUB_STEP_SUMMARY"
  fi
fi

# Say once, in the plan, when the push's base was unavailable.
if [[ "$MODE" == plan* && -f "$reports/base.txt" && -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  printf '%s\n\n' "$(cat "$reports/base.txt")" >> "$GITHUB_STEP_SUMMARY"
fi
exit "$status"
