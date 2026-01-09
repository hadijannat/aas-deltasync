#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CERT_DIR="$SCRIPT_DIR/certs"

mkdir -p "$CERT_DIR"

if [[ -f "$CERT_DIR/ca.crt" ]]; then
  echo "Certificates already exist in $CERT_DIR"
  echo "Remove the directory to regenerate."
  exit 0
fi

echo "Generating demo CA..."
openssl genrsa -out "$CERT_DIR/ca.key" 4096
openssl req -x509 -new -nodes -key "$CERT_DIR/ca.key" \
  -sha256 -days 3650 -subj "/CN=aas-deltasync-demo-ca" \
  -out "$CERT_DIR/ca.crt"

echo "Generating broker key and certificate..."
openssl genrsa -out "$CERT_DIR/server.key" 2048
openssl req -new -key "$CERT_DIR/server.key" \
  -out "$CERT_DIR/server.csr" \
  -config "$SCRIPT_DIR/openssl.cnf"

openssl x509 -req -in "$CERT_DIR/server.csr" \
  -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial \
  -out "$CERT_DIR/server.crt" -days 825 -sha256 \
  -extensions req_ext -extfile "$SCRIPT_DIR/openssl.cnf"

echo "Done. CA certificate: $CERT_DIR/ca.crt"
