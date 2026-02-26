#!/bin/bash
# Rauthy initial setup for jimeng-gateway
#
# After `docker compose up -d`, Rauthy auto-creates admin account.
# This script registers an OIDC client for the gateway Dashboard.
#
# Prerequisites:
#   1. docker compose up -d (rauthy running on :8080)
#   2. Wait ~5s for init
#
# Rauthy admin UI: http://localhost:8080/auth/v1/admin
# Login: admin@localhost / changeme123

set -euo pipefail

RAUTHY_URL="http://localhost:8080"
CLIENT_ID="jimeng-gateway"
CLIENT_NAME="Jimeng Gateway Dashboard"
REDIRECT_URI="http://localhost:5100/auth/callback"

echo "=== Rauthy Setup for jimeng-gateway ==="
echo ""
echo ">>> Rauthy Admin UI: ${RAUTHY_URL}/auth/v1/admin"
echo ">>> Default login: admin@localhost / changeme123"
echo ""
echo "Please create an OIDC client manually in the Admin UI:"
echo ""
echo "  1. Open ${RAUTHY_URL}/auth/v1/admin"
echo "  2. Login with admin credentials"
echo "  3. Go to 'Clients' → 'New Client'"
echo "  4. Client ID: ${CLIENT_ID}"
echo "  5. Client Name: ${CLIENT_NAME}"
echo "  6. Redirect URIs: ${REDIRECT_URI}"
echo "  7. Scopes: openid email profile"
echo "  8. Save and copy the Client Secret"
echo ""
echo "Then add to your gateway .env:"
echo ""
echo "  AUTH_ENABLED=true"
echo "  OIDC_ISSUER_URL=${RAUTHY_URL}/auth/v1"
echo "  OIDC_CLIENT_ID=${CLIENT_ID}"
echo "  OIDC_CLIENT_SECRET=<secret from Rauthy>"
echo "  OIDC_REDIRECT_URL=${REDIRECT_URI}"
echo ""
echo "=== Done ==="
