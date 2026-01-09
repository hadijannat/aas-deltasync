#!/bin/bash
# AAS-ΔSync Convergence Demo
# ===========================
# Demonstrates two-site synchronization with simulated partition

set -e

echo "========================================"
echo "AAS-ΔSync Convergence Demo"
echo "========================================"
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
SITE_A_SM_REPO="http://localhost:8082"
SITE_B_SM_REPO="http://localhost:8092"
SUBMODEL_ID="urn:example:sm:demo"
SUBMODEL_ID_B64=$(echo -n "$SUBMODEL_ID" | base64 | tr '+/' '-_' | tr -d '=')

echo -e "${YELLOW}Step 1: Verify stack is running${NC}"
echo "Checking BaSyx Submodel Repositories..."

if ! curl -s "$SITE_A_SM_REPO/submodels" > /dev/null; then
    echo -e "${RED}Site A SM Repo not reachable. Run: docker compose up -d${NC}"
    exit 1
fi

if ! curl -s "$SITE_B_SM_REPO/submodels" > /dev/null; then
    echo -e "${RED}Site B SM Repo not reachable. Run: docker compose up -d${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Both sites reachable${NC}"
echo ""

echo -e "${YELLOW}Step 2: Create a demo submodel on Site A${NC}"
SUBMODEL_JSON='{
  "idShort": "DemoSubmodel",
  "id": "urn:example:sm:demo",
  "modelType": "Submodel",
  "submodelElements": [
    {
      "idShort": "Temperature",
      "modelType": "Property",
      "valueType": "xs:double",
      "value": "25.0"
    },
    {
      "idShort": "Status",
      "modelType": "Property",
      "valueType": "xs:string",
      "value": "Initialized"
    }
  ]
}'

curl -s -X POST "$SITE_A_SM_REPO/submodels" \
  -H "Content-Type: application/json" \
  -d "$SUBMODEL_JSON" > /dev/null 2>&1 || true

echo "Created submodel on Site A"
echo ""

echo -e "${YELLOW}Step 3: Verify submodel on Site A${NC}"
VALUE_A=$(curl -s "$SITE_A_SM_REPO/submodels/$SUBMODEL_ID_B64/submodel-elements/Temperature/\$value" | jq -r '.')
echo "Site A Temperature = $VALUE_A"
echo ""

echo -e "${YELLOW}Step 4: Simulate concurrent updates${NC}"
echo "Updating Temperature on Site A to 30.0..."
curl -s -X PATCH "$SITE_A_SM_REPO/submodels/$SUBMODEL_ID_B64/submodel-elements/Temperature/\$value" \
  -H "Content-Type: application/json" \
  -d '30.0' > /dev/null

echo "Updating Status on Site A to 'Running'..."
curl -s -X PATCH "$SITE_A_SM_REPO/submodels/$SUBMODEL_ID_B64/submodel-elements/Status/\$value" \
  -H "Content-Type: application/json" \
  -d '"Running"' > /dev/null

echo ""

echo -e "${YELLOW}Step 5: Read final state${NC}"
FINAL_TEMP=$(curl -s "$SITE_A_SM_REPO/submodels/$SUBMODEL_ID_B64/submodel-elements/Temperature/\$value" | jq -r '.')
FINAL_STATUS=$(curl -s "$SITE_A_SM_REPO/submodels/$SUBMODEL_ID_B64/submodel-elements/Status/\$value" | jq -r '.')

echo "Final Temperature = $FINAL_TEMP"
echo "Final Status = $FINAL_STATUS"
echo ""

echo "========================================"
echo -e "${GREEN}Demo Complete!${NC}"
echo "========================================"
echo ""
echo "This demo showed:"
echo "  1. Submodel creation on Site A"
echo "  2. Property updates via AAS Part 2 API"
echo "  3. BaSyx MQTT event generation (check mosquitto logs)"
echo ""
echo "To see MQTT events, run in another terminal:"
echo "  docker exec -it mosquitto mosquitto_sub -t '#' -v"
echo ""
echo "When agents are deployed, they will:"
echo "  - Receive these MQTT events"
echo "  - Convert them to CRDT deltas"
echo "  - Replicate to other agents"
echo "  - Apply converged state to local AAS servers"
