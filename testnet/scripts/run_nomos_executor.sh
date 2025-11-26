#!/bin/sh

set -e

export CFG_FILE_PATH="/config.yaml" \
       CFG_SERVER_ADDR="${CFG_SERVER_ADDR:-http://cfgsync:4400}" \
       CFG_HOST_IP=$(hostname -i) \
        CFG_HOST_KIND="${CFG_HOST_KIND:-executor}" \
        CFG_HOST_IDENTIFIER="${CFG_HOST_IDENTIFIER:-executor-$(hostname -i)}" \
       LOG_LEVEL="INFO" \
       POL_PROOF_DEV_MODE=true

# Ensure recovery directory exists to avoid early crashes in services that
# persist state.
mkdir -p /recovery

/usr/bin/cfgsync-client && \
    exec /usr/bin/nomos-executor /config.yaml
