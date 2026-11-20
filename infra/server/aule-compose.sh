#!/bin/bash
# Runs docker compose for Aules tutor server with this servers settings.
export AULE_ENV_FILE=/etc/aule/aule.env
export AULE_DB_PORT=5434
export AULE_AGENT_MEMORY=6g
export AULE_UID=1001 AULE_GID=1001
export AULE_FCM_KEY_FILE=/etc/aule/firebase.json
cd /opt/aule-sw && exec docker compose -p aule-sw "$@"
