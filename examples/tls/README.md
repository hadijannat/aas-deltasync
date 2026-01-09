# TLS Demo (MQTTS)

This demo runs the standard AAS-ΔSync topology with a TLS-enabled MQTT broker.

## Prerequisites

- Docker + Docker Compose
- OpenSSL

## Steps

1) Generate demo certificates:

```bash
./examples/tls/generate-certs.sh
```

2) Start the TLS demo stack:

```bash
docker compose -f examples/tls/docker-compose.yml up -d
```

3) (Optional) Verify broker connectivity with TLS:

```bash
mosquitto_sub -h localhost -p 8883 --cafile examples/tls/certs/ca.crt -t 'sm-repository/#' -v
```

## Notes

- The demo uses `mqtts://` with port `8883` and a self-signed CA.
- Agents read the CA certificate from `DELTASYNC_MQTT_CA_PATH` (mounted at `/certs/ca.crt`).
- To regenerate certificates, remove `examples/tls/certs/` and re-run the script.
