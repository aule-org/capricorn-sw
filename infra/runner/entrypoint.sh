#!/usr/bin/env bash
set -euo pipefail
cd /home/runner

if [[ ! -f .runner ]]; then
  : "${RUNNER_REGISTRATION_TOKEN:?A short-lived organization runner registration token is required}"
  ./config.sh --unattended --url https://github.com/meet-aule \
    --token "$RUNNER_REGISTRATION_TOKEN" --name aule-quality \
    --runnergroup aule-quality --labels aule-quality --work _work
fi
unset RUNNER_REGISTRATION_TOKEN
exec ./run.sh
