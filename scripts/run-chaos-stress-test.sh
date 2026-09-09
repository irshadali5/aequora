#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "=========================================================================="
echo "  Aequora Automated High-Load & Multi-Chaos Orchestration Runner"
echo "=========================================================================="

manifest_path=""
client_pid=""

cleanup() {
    if [ -n "$client_pid" ]; then
        kill "$client_pid" >/dev/null 2>&1 || true
        wait "$client_pid" >/dev/null 2>&1 || true
    fi
    echo -e "\n[CLEANUP] Tearing down Kubernetes pod..."
    if [ -n "$manifest_path" ]; then
        podman play kube --down "$manifest_path" >/dev/null 2>&1 || true
        rm -f "$manifest_path"
    fi
}
trap cleanup EXIT

command -v podman >/dev/null
command -v curl >/dev/null
command -v od >/dev/null
command -v sed >/dev/null
command -v tr >/dev/null
test -x ./target/release/examples/realworld_stress

manifest_path="$(mktemp /tmp/aequora-stress-manifest.XXXXXX.yaml)"
export AEQUORA_STRESS_AUTH_KEY_HEX
AEQUORA_STRESS_AUTH_KEY_HEX="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
test "${#AEQUORA_STRESS_AUTH_KEY_HEX}" -eq 64
sed "s/AEQUORA_STRESS_AUTH_KEY_HEX_VALUE/$AEQUORA_STRESS_AUTH_KEY_HEX/" \
    deploy/kubernetes/realworld-k8s.yaml >"$manifest_path"

echo -e "\n[STEP 1] Deploying Aequora Server via podman play kube..."
podman play kube "$manifest_path"

echo -e "\n[STEP 2] Waiting for readiness probe (HTTP 204)..."
ready=0
for attempt in {1..30}; do
    if [ "$(curl --silent --show-error --max-time 1 --output /dev/null --write-out "%{http_code}" \
        --proto '=http' http://127.0.0.1:8443/sync/v1/health/ready || true)" = "204" ]; then
        echo "Server is READY and healthy! (Attempt $attempt)"
        ready=1
        break
    fi
    sleep 0.5
done

if [ "$ready" -ne 1 ]; then
    echo "ERROR: Server failed readiness probe within 15 seconds" >&2
    podman logs aequora-stress-harness-pod-aequora-api || true
    exit 1
fi

CONTAINER_NAME="aequora-stress-harness-pod-aequora-api"

echo -e "\n=========================================================================="
echo "  EXPERIMENT 1: HIGH LOAD + HOT-KEY CONTENTION + ADVERSARIAL ATTACKS"
echo "=========================================================================="
echo "Running 40 concurrent legitimate clients + 8 hot-keys + 8 rogue attack threads..."
./target/release/examples/realworld_stress \
    --url http://127.0.0.1:8443 \
    --concurrency 40 \
    --duration 30 \
    --batch-size 25 \
    --hot-keys 8 \
    --adversarial-clients 8 \
    --skip-functional

echo -e "\nContainer resource stats after Experiment 1:"
podman stats --no-stream "$CONTAINER_NAME"

echo -e "\n=========================================================================="
echo "  EXPERIMENT 2: HYPERVISOR / NODE FREEZE & RECONNECT STORM (podman pause)"
echo "=========================================================================="
echo "Launching 40 concurrent clients for 25 seconds with runtime freeze at T+6s..."

# Launch stress client in background
./target/release/examples/realworld_stress \
    --url http://127.0.0.1:8443 \
    --concurrency 40 \
    --duration 25 \
    --batch-size 20 \
    --skip-functional &
CLIENT_PID=$!
client_pid="$CLIENT_PID"

sleep 6
echo -e "\n>>> [CHAOS INJECTION] Freezing container with 'podman pause' (simulating 2.5s network stall / hypervisor freeze)..."
podman pause "$CONTAINER_NAME"
podman ps --filter "name=$CONTAINER_NAME"

sleep 2.5
echo -e "\n>>> [CHAOS CLEAR] Resuming container with 'podman unpause' (triggering massive client reconnect burst)..."
podman unpause "$CONTAINER_NAME"

wait "$CLIENT_PID"
client_pid=""

echo -e "\nContainer resource stats after Experiment 2:"
podman stats --no-stream "$CONTAINER_NAME"

echo -e "\n=========================================================================="
echo "  EXPERIMENT 3: CRASH RECOVERY & MID-FLIGHT RESTART (podman restart)"
echo "=========================================================================="
echo "Launching 40 concurrent clients for 25 seconds with container restart at T+8s..."

./target/release/examples/realworld_stress \
    --url http://127.0.0.1:8443 \
    --concurrency 40 \
    --duration 25 \
    --batch-size 20 \
    --skip-functional &
RESTART_CLIENT_PID=$!
client_pid="$RESTART_CLIENT_PID"

sleep 8
echo -e "\n>>> [CHAOS INJECTION] Restarting server container under full client flight..."
podman restart "$CONTAINER_NAME" >/dev/null

wait "$RESTART_CLIENT_PID"
client_pid=""

echo -e "\nContainer resource stats after Experiment 3:"
podman stats --no-stream "$CONTAINER_NAME"

echo -e "\n=========================================================================="
echo "  ALL HIGH-LOAD STRESS & CHAOS/INTERFERENCE EXPERIMENTS COMPLETED SUCCESSFULLY!"
echo "=========================================================================="
