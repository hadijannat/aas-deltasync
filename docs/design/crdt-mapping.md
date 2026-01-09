# CRDT Mapping for AAS Elements

This document defines the deterministic conflict resolution rules for mapping AAS submodel elements to CRDT structures.

## Overview

AAS-ΔSync uses a **delta-state CRDT** approach where:
- The submodel element tree is represented as an **Observed-Remove Map** (OR-Map)
- Each leaf value is stored in a **Last-Writer-Wins register** (LWW)
- Timestamps use **Hybrid Logical Clocks** (HLC) with actor ID tiebreakers

## Document Identity

Each CRDT document is uniquely identified by:

```
DocId = (AAS Identifier, Submodel Identifier, View)
```

Where `View` is one of:
- `normal` - Full submodel structure
- `$value` - Value-only view (preferred for replication)
- `$metadata` - Metadata-only view

## Merge Strategies by Element Type

| Element Type | Strategy | Details |
|--------------|----------|---------|
| **Property** | LWW Register | Single value, timestamp + actor tiebreaker |
| **Range** | Per-field LWW | Separate registers for `min` and `max` |
| **MultiLanguageProperty** | Per-language LWW | Separate register per language code |
| **SubmodelElementCollection** | OR-Map | Children keyed by idShort |
| **SubmodelElementList** | Stable ID OR-Map | Elements keyed by stable UUID, not index |
| **File/Blob** | Content-addressed LWW | Pointer to immutable blob ID |
| **ReferenceElement** | LWW Register | Reference keys as single value |
| **Entity** | OR-Map | Entity statements as OR-Map entries |

## Canonical Path Format

Paths are formatted as `idShortPath` per AAS Part 2:

```
TechnicalData.MaxTemperature
Components[stable-uuid-123].Weight
ContactInformation.Phone[Business].AreaCode
```

**Important**: For `SubmodelElementList`, we do NOT use numeric indices as keys because indices can shift under concurrent insertions. Instead, each list element is assigned a stable UUID at creation time.

## Timestamp Ordering

Timestamps are totally ordered using:

1. **Physical time** (milliseconds since epoch)
2. **Logical counter** (for events at same physical time)
3. **Actor ID** (UUID, for deterministic tiebreaking)

```rust
impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.physical_ms.cmp(&other.physical_ms)
            .then(self.logical.cmp(&other.logical))
            .then(self.actor_id.cmp(&other.actor_id))
    }
}
```

## Delta Representation

Deltas are compact representations of state changes:

```rust
struct Delta<K, V> {
    inserts: Vec<(K, V, Timestamp)>,  // Path → Value at Timestamp
    removes: Vec<(K, Timestamp)>,     // Path removed at Timestamp
}
```

Deltas are:
- **Idempotent**: Applying the same delta twice has no additional effect
- **Commutative**: Order of delta application doesn't affect final state
- **Associative**: Grouping of delta merges doesn't affect result

## Tombstone Handling

Removed entries are tracked via tombstones until compaction:

```rust
tombstones: HashMap<Path, Timestamp>
```

An insert is ignored if there's a tombstone with a higher or equal timestamp. Tombstones can be garbage collected after all peers have synced past that timestamp.

## Example: Concurrent Property Update

```
Site A:                          Site B:
                                 
t=1000: X = 10                   
                                 t=1001: X = 20
                                 
--- Network partition heals ---

Both sites merge deltas:
- Site A receives: set(X, 20, t=1001@B)
- Site B receives: set(X, 10, t=1000@A)

Result (both sites):
X = 20 (because t=1001 > t=1000)
```

## Example: Add-Remove Conflict

```
Site A:                          Site B:

t=1000: add(Y, 5)                
                                 t=1001: remove(Y)
                                 
--- Merge ---

Result: Y is removed (remove timestamp t=1001 > add timestamp t=1000)
```

## Example: Concurrent Adds

```
Site A:                          Site B:

t=1000: add(Z, 100)              t=1000: add(Z, 200)
                                 
--- Merge ---

Result: Z = 200 (actor ID tiebreaker, B > A)
```

## References

- Shapiro et al., "Conflict-free Replicated Data Types" (2011)
- Almeida et al., "Delta State Replicated Data Types" (2018)
- Kulkarni et al., "Logical Physical Clocks" (2014)
- IDTA 2002-1-0: AAS Part 2 (HTTP/REST API)
